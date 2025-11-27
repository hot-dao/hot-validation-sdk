use serde::{Deserialize, Serialize};
use serde_with::DisplayFromStr;
use serde_with::base64::Base64;
use serde_with::serde_as;

#[serde_as]
#[derive(Serialize, Deserialize, Clone, Debug, schemars::JsonSchema, Eq, PartialEq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum CosmosInputData {
    HotVerify {
        #[serde_as(as = "DisplayFromStr")]
        #[schemars(with = "String")]
        nonce: u128,
        #[serde_as(as = "Base64")]
        #[schemars(with = "String")]
        msg_hash: [u8; 32],
    },
    IsExecuted {
        #[serde_as(as = "DisplayFromStr")]
        #[schemars(with = "String")]
        nonce: u128,
    },
}
