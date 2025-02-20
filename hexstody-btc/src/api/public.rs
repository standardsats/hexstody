use crate::constants::{WITHDRAWAL_CONFIRM_URI, WITHDRAWAL_REJECT_URI};
use crate::state::ScanState;
use bitcoin::{consensus::encode, network::constants::Network, Address, Amount, Transaction};
use bitcoincore_rpc::{Client, RpcApi};
use bitcoincore_rpc_json::{AddressType, GetTransactionResultDetailCategory};
use hexstody_api::domain::CurrencyAddress;
use hexstody_api::types::{
    ConfirmationData, ConfirmedWithdrawal, SignatureData, WithdrawalResponse,
};
use hexstody_api::types::{FeeResponse, HotBalanceResponse};
use hexstody_btc_api::bitcoin::*;
use hexstody_btc_api::events::*;
use hexstody_sig::verify_signature;
use log::*;
use p256::PublicKey;
use rocket::fairing::AdHoc;
use rocket::figment::{providers::Env, Figment};
use rocket::http::Status;
use rocket::serde::json;
use rocket::{get, post, serde::json::Json, Config, State};
use rocket_okapi::settings::UrlObject;
use rocket_okapi::{openapi, openapi_get_routes, rapidoc::*, swagger_ui::*};
use std::net::IpAddr;
use std::str::FromStr;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{Mutex, Notify};
use tokio::time::timeout;

use super::error;

#[openapi(tag = "misc")]
#[get("/ping")]
fn ping() -> Json<()> {
    Json(())
}

#[openapi(tag = "events")]
#[post("/events")]
async fn poll_events(
    polling_timeout: &State<Duration>,
    state: &State<Arc<Mutex<ScanState>>>,
    state_notify: &State<Arc<Notify>>,
) -> Json<BtcEvents> {
    trace!("Awaiting state events");
    match timeout(*polling_timeout.inner(), state_notify.notified()).await {
        Ok(_) => {
            info!("Got new events for deposit");
        }
        Err(_) => {
            trace!("No new events but releasing long poll");
        }
    }
    let mut state_rw = state.lock().await;
    let result = Json(BtcEvents {
        hash: state_rw.last_block.into(),
        height: state_rw.last_height,
        events: state_rw.events.clone(),
    });
    state_rw.events = vec![];
    result
}

#[openapi(tag = "deposit")]
#[post("/deposit/address")]
async fn get_deposit_address(client: &State<Client>) -> error::Result<BtcAddress> {
    let address = client
        .get_new_address(None, Some(AddressType::Bech32))
        .map_err(|e| error::Error::from(e))?;
    Ok(Json(address.into()))
}

#[openapi(tag = "fees")]
#[get("/fees")]
async fn get_fees(client: &State<Client>) -> Json<FeeResponse> {
    let est = client
        .estimate_smart_fee(2, None)
        .map_err(|e| error::Error::from(e));
    let res = FeeResponse {
        fee_rate: 5, // default 5 sat/byte
        block: None,
    };
    match est {
        Err(_) => Json(res),
        Ok(fe) => match fe.fee_rate {
            None => Json(res),
            Some(val) => Json(FeeResponse {
                fee_rate: val.as_sat(),
                block: Some(fe.blocks),
            }),
        },
    }
}

// Configuration for /withdraw handler
struct WithdrawCfg {
    min_confirmations: i16,
    op_public_keys: Vec<PublicKey>,
    hot_domain: String,
    network: Network,
}

#[openapi(tag = "withdraw")]
#[post("/withdraw", format = "json", data = "<cw>")]
async fn withdraw_btc(
    client: &State<Client>,
    cfg: &State<WithdrawCfg>,
    cw: Json<ConfirmedWithdrawal>,
) -> error::Result<WithdrawalResponse> {
    debug!("{:?}", cw);
    let WithdrawCfg {
        min_confirmations,
        op_public_keys,
        hot_domain,
        network,
    } = cfg.inner();
    let mut valid_confirms = 0;
    let mut valid_rejections = 0;
    let min_confirmations = min_confirmations.clone();
    let confirmation_data = ConfirmationData {
        id: cw.id,
        user: cw.user.clone(),
        address: cw.address.clone(),
        created_at: cw.created_at.clone(),
        amount: cw.amount,
    };
    let msg = json::to_string(&confirmation_data).unwrap();
    let confirm_url = [hot_domain.clone(), WITHDRAWAL_CONFIRM_URI.to_owned()].join("");
    let reject_url = [hot_domain.clone(), WITHDRAWAL_REJECT_URI.to_owned()].join("");
    let confirm_msg = [confirm_url, msg.clone()].join(":");
    let reject_msg = [reject_url, msg].join(":");
    let op_keys = Some(op_public_keys.clone());
    for sigdata in &cw.confirmations {
        let op_keys = op_keys.clone();
        let SignatureData {
            signature,
            nonce,
            public_key,
        } = sigdata;
        if verify_signature(op_keys, public_key, nonce, confirm_msg.clone(), signature).is_ok() {
            valid_confirms = valid_confirms + 1;
        };
    }
    for sigdata in &cw.rejections {
        let op_keys = op_keys.clone();
        let SignatureData {
            signature,
            nonce,
            public_key,
        } = sigdata;
        if verify_signature(op_keys, public_key, nonce, reject_msg.clone(), signature).is_ok() {
            valid_rejections = valid_rejections + 1;
        };
    }
    debug!(
        "Confirms/rejections: {}/{}",
        valid_confirms, valid_rejections
    );
    if (valid_confirms > valid_rejections)
        && (valid_confirms - valid_rejections >= min_confirmations)
    {
        if let CurrencyAddress::BTC(hexstody_api::domain::BtcAddress { addr }) = &cw.address {
            if let Ok(addr) = bitcoin::Address::from_str(addr.as_str()) {
                let comment = cw.id.to_string();
                let amount = bitcoin::Amount::from_sat(cw.amount);
                let txid = client
                    .send_to_address(&addr, amount, Some(&comment), None, None, None, None, None)
                    .map_err(|e| {
                        (
                            Status::InternalServerError,
                            Json(crate::api::error::ErrorMessage {
                                message: format!("Failed to post the tx: {:?}", e),
                                code: 500,
                            }),
                        )
                    })?;
                let tx = client.get_transaction(&txid, None).map_err(|e| {
                    (
                        Status::InternalServerError,
                        Json(crate::api::error::ErrorMessage {
                            message: format!("Tx not found: {:?}", e),
                            code: 500,
                        }),
                    )
                })?;
                let fee = tx.fee.map(|f| f.as_sat().abs() as u64);
                let input_addresses = tx
                    .details
                    .iter()
                    .map(|x| match x.category {
                        GetTransactionResultDetailCategory::Receive
                        | GetTransactionResultDetailCategory::Generate
                        | GetTransactionResultDetailCategory::Immature => {
                            x.address.clone().map(|a| CurrencyAddress::from(a))
                        }
                        _ => None,
                    })
                    .flatten()
                    .collect();
                let deserialized_tx: Transaction = encode::deserialize(&tx.hex).map_err(|e| {
                    (
                        Status::InternalServerError,
                        Json(crate::api::error::ErrorMessage {
                            message: format!("Failed to deserialize tx: {:?}", e),
                            code: 500,
                        }),
                    )
                })?;
                let output_addresses = deserialized_tx
                    .output
                    .iter()
                    .map(|out| Address::from_script(&out.script_pubkey, *network))
                    .flatten()
                    .map(|addr| CurrencyAddress::from(addr))
                    .collect();
                let resp = WithdrawalResponse {
                    id: cw.id.clone(),
                    txid: BtcTxid(txid),
                    fee,
                    input_addresses,
                    output_addresses,
                };
                debug!("OK: {:?}", resp);
                Ok(Json(resp))
            } else {
                Err((
                    Status::BadRequest,
                    Json(crate::api::error::ErrorMessage {
                        message: "Not BTC??".to_owned(),
                        code: 500,
                    }),
                ))
            }
        } else {
            Err((
                Status::BadRequest,
                Json(crate::api::error::ErrorMessage {
                    message: "Not BTC??".to_owned(),
                    code: 500,
                }),
            ))
        }
    } else {
        Err((
            Status::Forbidden,
            Json(crate::api::error::ErrorMessage {
                message: "Signature verification failed".to_owned(),
                code: 403,
            }),
        ))
    }
}

#[openapi(tag = "withdraw")]
#[post("/withdraw/under", format = "json", data = "<cw>")]
async fn withdraw_btc_under_limit(
    client: &State<Client>,
    cfg: &State<WithdrawCfg>,
    cw: Json<ConfirmedWithdrawal>,
) -> error::Result<WithdrawalResponse> {
    let network = cfg.network;
    if let CurrencyAddress::BTC(hexstody_api::domain::BtcAddress { addr }) = &cw.address {
        if let Ok(addr) = bitcoin::Address::from_str(addr.as_str()) {
            let comment = cw.id.to_string();
            let amount = bitcoin::Amount::from_sat(cw.amount);
            let txid = client
                .send_to_address(&addr, amount, Some(&comment), None, None, None, None, None)
                .map_err(|e| {
                    (
                        Status::InternalServerError,
                        Json(crate::api::error::ErrorMessage {
                            message: format!("Failed to post the tx: {:?}", e),
                            code: 500,
                        }),
                    )
                })?;
            let tx = client.get_transaction(&txid, None).map_err(|e| {
                (
                    Status::InternalServerError,
                    Json(crate::api::error::ErrorMessage {
                        message: format!("Tx not found: {:?}", e),
                        code: 500,
                    }),
                )
            })?;
            let fee = tx.fee.map(|f| f.as_sat().abs() as u64);
            let input_addresses = tx
                .details
                .iter()
                .map(|x| match x.category {
                    GetTransactionResultDetailCategory::Receive
                    | GetTransactionResultDetailCategory::Generate
                    | GetTransactionResultDetailCategory::Immature => {
                        x.address.clone().map(|a| CurrencyAddress::from(a))
                    }
                    _ => None,
                })
                .flatten()
                .collect();
            let deserialized_tx: Transaction = encode::deserialize(&tx.hex).map_err(|e| {
                (
                    Status::InternalServerError,
                    Json(crate::api::error::ErrorMessage {
                        message: format!("Failed to deserialize tx: {:?}", e),
                        code: 500,
                    }),
                )
            })?;
            let output_addresses = deserialized_tx
                .output
                .iter()
                .map(|out| Address::from_script(&out.script_pubkey, network))
                .flatten()
                .map(|addr| CurrencyAddress::from(addr))
                .collect();
            let resp = WithdrawalResponse {
                id: cw.id.clone(),
                txid: BtcTxid(txid),
                fee,
                input_addresses,
                output_addresses,
            };
            debug!("OK: {:?}", resp);
            Ok(Json(resp))
        } else {
            Err((
                Status::BadRequest,
                Json(crate::api::error::ErrorMessage {
                    message: "Not BTC??".to_owned(),
                    code: 500,
                }),
            ))
        }
    } else {
        Err((
            Status::BadRequest,
            Json(crate::api::error::ErrorMessage {
                message: "Not BTC??".to_owned(),
                code: 500,
            }),
        ))
    }
}

#[openapi(tag = "Hot wallet balance")]
#[post("/hot-wallet-balance")]
async fn get_hot_wallet_balance(client: &State<Client>) -> error::Result<HotBalanceResponse> {
    client
        .get_balance(None, None)
        .map_err(|e| {
            (
                Status::InternalServerError,
                Json(crate::api::error::ErrorMessage {
                    message: format!("Failed get the balance: {:?}", e),
                    code: 500,
                }),
            )
        })
        .map(|a| {
            Json(HotBalanceResponse {
                balance: a.as_sat(),
            })
        })
}

async fn guard_regtest(client: &Client) -> error::Result<()> {
    let info = client.get_blockchain_info().expect("got blockchain info");
    if info.chain == "regtest" {
        Ok(Json(()))
    } else {
        Err(error::Error::DebugNotEnabled.into())
    }
}

#[openapi(tag = "regtest")]
#[post("/generate?<blocks>")]
async fn generate_blocks(client: &State<Client>, blocks: Option<u16>) -> error::Result<()> {
    guard_regtest(client).await?;

    let to_generate = blocks.unwrap_or(1);

    let address = client
        .get_new_address(None, None)
        .map_err(|e| error::Error::NodeRpc(e))?;
    for _ in 0..to_generate {
        client
            .generate_to_address(1, &address)
            .map_err(|e| error::Error::NodeRpc(e))?;
    }
    Ok(Json(()))
}

#[openapi(tag = "regtest")]
#[post("/newaddress")]
async fn new_address(client: &State<Client>) -> error::Result<String> {
    guard_regtest(client).await?;
    let address = client
        .get_new_address(None, None)
        .map_err(|e| error::Error::NodeRpc(e))?;
    Ok(Json(address.to_string()))
}

#[openapi(tag = "regtest")]
#[post("/send/<address>/<amount>")]
async fn send_to_address(
    client: &State<Client>,
    address: String,
    amount: u64,
) -> error::Result<()> {
    guard_regtest(client).await?;
    let parsed_address = Address::from_str(&address).map_err(|e| error::Error::AddressParse(e))?;
    client
        .send_to_address(
            &parsed_address,
            Amount::from_sat(amount),
            None,
            None,
            None,
            Some(true),
            None,
            None,
        )
        .map_err(|e| error::Error::NodeRpc(e))?;
    Ok(Json(()))
}

pub async fn serve_public_api(
    btc: Client,
    address: IpAddr,
    port: u16,
    start_notify: Arc<Notify>,
    state: Arc<Mutex<ScanState>>,
    state_notify: Arc<Notify>,
    polling_duration: Duration,
    secret_key: Option<&str>,
    op_public_keys: Vec<PublicKey>,
    min_confirmations: i16,
    hot_domain: String,
    network: Network,
) -> Result<(), rocket::Error> {
    let zero_key =
        "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA==";
    let secret_key = secret_key.unwrap_or(zero_key);
    let figment = Figment::from(Config {
        address,
        port,
        ..Config::default()
    })
    .merge(("secret_key", secret_key))
    .merge(Env::prefixed("HEXSTODY_BTC_").global());

    let on_ready = AdHoc::on_liftoff("API Start!", |_| {
        Box::pin(async move {
            start_notify.notify_one();
        })
    });

    let withdraw_cfg = WithdrawCfg {
        min_confirmations,
        op_public_keys,
        hot_domain,
        network,
    };
    let _ = rocket::custom(figment)
        .mount(
            "/",
            openapi_get_routes![
                ping,
                poll_events,
                get_deposit_address,
                get_fees,
                withdraw_btc,
                withdraw_btc_under_limit,
                generate_blocks,
                new_address,
                send_to_address,
                get_hot_wallet_balance,
            ],
        )
        .mount(
            "/swagger/",
            make_swagger_ui(&SwaggerUIConfig {
                url: "../openapi.json".to_owned(),
                ..Default::default()
            }),
        )
        .mount(
            "/rapidoc/",
            make_rapidoc(&RapiDocConfig {
                general: GeneralConfig {
                    spec_urls: vec![UrlObject::new("General", "../openapi.json")],
                    ..Default::default()
                },
                hide_show: HideShowConfig {
                    allow_spec_url_load: false,
                    allow_spec_file_load: false,
                    ..Default::default()
                },
                ..Default::default()
            }),
        )
        .manage(polling_duration)
        .manage(state)
        .manage(state_notify)
        .manage(btc)
        .manage(withdraw_cfg)
        .attach(on_ready)
        .launch()
        .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use hexstody_btc_test::runner::*;

    #[tokio::test]
    async fn test_public_api_ping() {
        run_test(|_, api| async move {
            api.ping().await.unwrap();
        })
        .await;
    }

    #[tokio::test]
    async fn test_public_api_address() {
        run_test(|_, api| async move {
            assert!(api.deposit_address().await.is_ok());
        })
        .await;
    }
}
