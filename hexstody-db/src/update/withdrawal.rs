use p256::{ecdsa::Signature, PublicKey};
use serde::{Deserialize, Serialize};

use crate::state::withdraw::WithdrawalRequestId;
use crate::update::signup::UserId;
use hexstody_api::domain::{Currency, CurrencyAddress};
use hexstody_api::types::{
    ConfirmationData, SignatureData, WithdrawalRequestInfo as WithdrawalRequestInfoApi,
};

#[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
pub struct WithdrawalRequestInfo {
    /// User which initiated withdrawal request
    pub user: UserId,
    /// Receiving address
    pub address: CurrencyAddress,
    /// Amount of tokens to transfer
    pub amount: u64,
}

impl From<WithdrawalRequestInfoApi> for WithdrawalRequestInfo {
    fn from(value: WithdrawalRequestInfoApi) -> WithdrawalRequestInfo {
        WithdrawalRequestInfo {
            user: value.user,
            address: value.address,
            amount: value.amount,
        }
    }
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
pub enum WithdrawalRequestDecisionType {
    Confirm,
    Reject,
}

// This data type is used to create DB state update
#[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
pub struct WithdrawalRequestDecisionInfo {
    /// User which initiated withdrawal request
    pub user_id: UserId,
    /// Withdrawal request currency
    pub currency: Currency,
    /// Withdrawal request ID
    pub request_id: WithdrawalRequestId,
    /// API URL wich was used to send the decision
    pub url: String,
    /// Confirmaiton message
    pub msg: String,
    /// Operator's digital signature
    pub signature: Signature,
    /// Nonce that was generated during decision
    pub nonce: u64,
    /// Operator's public key corresponding to the signing private key
    pub public_key: PublicKey,
    /// Decision type: confirm or reject
    pub decision_type: WithdrawalRequestDecisionType,
}

impl
    From<(
        ConfirmationData,
        SignatureData,
        WithdrawalRequestDecisionType,
        String,
        String,
    )> for WithdrawalRequestDecisionInfo
{
    fn from(
        value: (
            ConfirmationData,
            SignatureData,
            WithdrawalRequestDecisionType,
            String,
            String,
        ),
    ) -> WithdrawalRequestDecisionInfo {
        WithdrawalRequestDecisionInfo {
            user_id: value.0 .0.user,
            currency: value.0 .0.address.currency(),
            request_id: value.0 .0.id,
            url: value.3,
            msg: value.4,
            signature: value.1.signature,
            nonce: value.1.nonce,
            public_key: value.1.public_key,
            decision_type: value.2,
        }
    }
}

/// This data type is what actually stored in DB.
/// Contains information required to check operator's digital signature.
#[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
pub struct WithdrawalRequestDecision {
    /// API URL wich was used to send the decision
    pub url: String,
    /// Confirmaiton message
    pub msg: String,
    /// Operator's digital signature
    pub signature: Signature,
    /// Nonce that was generated during decision
    pub nonce: u64,
    /// Operator's public key corresponding to the signing private key
    pub public_key: PublicKey,
}

impl From<WithdrawalRequestDecisionInfo> for WithdrawalRequestDecision {
    fn from(info: WithdrawalRequestDecisionInfo) -> WithdrawalRequestDecision {
        WithdrawalRequestDecision {
            url: info.url,
            msg: info.msg,
            signature: info.signature,
            nonce: info.nonce,
            public_key: info.public_key,
        }
    }
}
