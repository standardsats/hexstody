use crate::state::ScanState;
use log::*;
use rocket_okapi::{openapi, openapi_get_routes, rapidoc::*, swagger_ui::*};
use rocket_okapi::settings::UrlObject;
use rocket::{get, post, serde::json::Json, Config, State};
use rocket::fairing::AdHoc;
use rocket::figment::{providers::Env, Figment};
use std::net::IpAddr;
use std::sync::Arc;
use super::types::*;
use tokio::sync::{Mutex, Notify};

#[openapi(tag = "misc")]
#[get("/ping")]
fn ping() -> Json<()> {
    Json(())
}

#[openapi(tag = "events")]
#[post("/events/deposit")]
async fn deposit_events(
    state: &State<Arc<Mutex<ScanState>>>,
    state_notify: &State<Arc<Notify>>,
) -> Json<DepositEvents> {
    info!("Awaiting state events");
    state_notify.notified().await;
    info!("Got new events for deposit");
    let mut state_rw = state.lock().await;
    let result = Json(DepositEvents {
        events: state_rw.deposit_events.clone(),
    });
    state_rw.deposit_events = vec![];
    result
}

pub async fn serve_public_api(
    address: IpAddr,
    port: u16,
    start_notify: Arc<Notify>,
    state: Arc<Mutex<ScanState>>,
    state_notify: Arc<Notify>,
) -> Result<(), rocket::Error> {
    let figment = Figment::from(Config {
        address,
        port,
        ..Config::default()
    })
    .merge(Env::prefixed("HEXSTODY_BTC_").global());

    let on_ready = AdHoc::on_liftoff("API Start!", |_| {
        Box::pin(async move {
            start_notify.notify_one();
        })
    });

    rocket::custom(figment)
        .mount("/", openapi_get_routes![ping, deposit_events])
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
        .manage(state)
        .manage(state_notify)
        .attach(on_ready)
        .launch()
        .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures::Future;
    use futures::FutureExt;
    use futures_util::future::TryFutureExt;
    use hexstody_btc_client::client::BtcClient;
    use std::panic::AssertUnwindSafe;

    const SERVICE_TEST_PORT: u16 = 8289;
    const SERVICE_TEST_HOST: &str = "127.0.0.1";

    async fn run_api_test<F, Fut>(test_body: F)
    where
        F: FnOnce() -> Fut,
        Fut: Future<Output = ()>,
    {
        let _ = env_logger::builder().is_test(true).try_init();
        let start_notify = Arc::new(Notify::new());

        let (sender, receiver) = tokio::sync::oneshot::channel();
        tokio::spawn({
            let start_notify = start_notify.clone();
            let state = Arc::new(Mutex::new(ScanState::default()));
            let state_notify = Arc::new(Notify::new());
            async move {
                let serve_task = serve_public_api(
                    SERVICE_TEST_HOST.parse().unwrap(),
                    SERVICE_TEST_PORT,
                    start_notify,
                    state,
                    state_notify,
                );
                futures::pin_mut!(serve_task);
                futures::future::select(serve_task, receiver.map_err(drop)).await;
            }
        });
        start_notify.notified().await;
        let res = AssertUnwindSafe(test_body()).catch_unwind().await;

        sender.send(()).unwrap();

        assert!(res.is_ok());
    }

    #[tokio::test]
    async fn test_public_api_ping() {
        run_api_test(|| async {
            let client = BtcClient::new(&format!(
                "http://{}:{}",
                SERVICE_TEST_HOST, SERVICE_TEST_PORT
            ));
            client.ping().await.unwrap();
        })
        .await;
    }
}
