use crate::internals::{SingleVerifier, VerifyArgs};
use anyhow::Result;
use anyhow::{bail, ensure, Context};
use async_trait::async_trait;
use base64::prelude::BASE64_STANDARD;
use base64::Engine;
use derive_more::{Deref, From};
use serde::{de, Deserialize, Deserializer, Serialize, Serializer};
use serde_hex::SerHex;
use serde_json::json;
use std::sync::Arc;
use tonlib_core::cell::{ArcCell, BagOfCells, CellBuilder};
use tonlib_core::TonAddress;

// access_list=[WalletAccessModel(account_id='EQANEViM3AKQzi6Aj3sEeyqFu8pXqhy9Q9xGoId_0qp3CNVJ',
// metadata='[{"method": "get_deposit_jetton_address", "args": ["int"]}, {"method": "verify_withdraw",
// "args": ["slice"]}]', chain_id=1117, msg=None)
// ] key_gen=1

#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
enum StepArgType {
    Int,
    Slice,
}

#[derive(Debug, Deserialize)]
struct ValidationStep {
    method: String,
    args: Vec<StepArgType>,
}

/// The validation schema that comes from the `auth.hot.tg` contract. It describes how we should wrap the data
#[derive(Debug, Deserialize, Deref, From)]
struct ValidationSchema(Vec<ValidationStep>);

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type", content = "value")]
enum StackItem {
    #[serde(rename = "num")]
    Num(String),
    #[serde(rename = "slice")]
    Slice(#[serde(deserialize_with = "DeserializableCell::from_base64")] DeserializableCell),
    #[serde(rename = "cell")]
    Cell(#[serde(deserialize_with = "DeserializableCell::from_base64")] DeserializableCell),
}

impl StackItem {
    pub fn as_num(&self) -> Result<String> {
        match self {
            StackItem::Num(n) => Ok(n.clone()),
            _ => Err(anyhow::anyhow!("stack item is not a number")),
        }
    }

    pub fn as_slice(&self) -> Result<ArcCell> {
        match self {
            StackItem::Slice(s) => Ok(s.0.clone()),
            _ => Err(anyhow::anyhow!("stack item is not a slice")),
        }
    }

    pub fn as_cell(&self) -> Result<ArcCell> {
        match self {
            StackItem::Cell(c) => Ok(c.0.clone()),
            _ => Err(anyhow::anyhow!("stack item is not a cell")),
        }
    }

    pub fn from_input_and_step(input: &str, step_arg_type: StepArgType) -> Result<StackItem> {
        match step_arg_type {
            StepArgType::Int => Ok(StackItem::Num(input.to_string())),
            StepArgType::Slice => {
                let bytes = hex::decode(input)?;

                let cell = CellBuilder::new().store_slice(&bytes)?.build()?;

                Ok(StackItem::Slice(
                    DeserializableCell(ArcCell::new(cell)),
                ))
            }
        }
    }
}

#[derive(Debug, Deserialize)]
struct TonResponse {
    stack: Vec<StackItem>,
}

#[derive(Debug, Deref, From)]
struct DeserializableCell(ArcCell);
impl DeserializableCell {
    fn from_base64<'de, D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        let s = s.trim();
        let boc = BagOfCells::parse_base64(s)
            .map_err(|e| de::Error::custom(format!("base64 parse: {e}")))?;
        if boc.roots.len() != 1 {
            return Err(de::Error::custom("expected exactly one root in boc"));
        }
        let cell = boc.roots[0].clone();
        Ok(DeserializableCell(cell))
    }
}
/// The type still has to implement `Deserialize`, even though we supply our own deserializer.
impl<'a> Deserialize<'a> for DeserializableCell {
    fn deserialize<D>(_deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: Deserializer<'a>,
    {
        unimplemented!()
    }
}
impl Serialize for DeserializableCell {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let bytes = BagOfCells::from_root(self.0.as_ref().clone())
            .serialize(false)
            .map_err(|e| serde::ser::Error::custom(format!("base64 serialize: {e}")))?;
        BASE64_STANDARD.encode(bytes).serialize(serializer)
    }
}

struct TonSingleVerifier {
    client: Arc<reqwest::Client>,
    server: String,
}

impl TonSingleVerifier {
    fn new(client: Arc<reqwest::Client>, server: String) -> Self {
        Self { client, server }
    }

    async fn make_call(
        &self,
        address: &TonAddress,
        method: &str,
        stack: Vec<StackItem>,
    ) -> Result<TonResponse> {
        let json = json!({
            "address": address.to_base64_url(),
            "method": method,
            "stack": stack,
        });
        let url = format!("{}/runGetMethod", self.server);

        let response: TonResponse = self
            .client
            .post(url)
            .json(&json)
            .send()
            .await?
            .json()
            .await?;

        Ok(response)
    }

    async fn do_intermediate_step(
        &self,
        address: &TonAddress,
        input: &str,
        step: &ValidationStep,
    ) -> Result<TonAddress> {
        ensure!(
            step.args.len() == 1,
            "Invalid step args number, expected 1, got {}",
            step.args.len()
        );
        let stack_item = StackItem::from_input_and_step(input, step.args[0])?;
        let response = self
            .make_call(address, &step.method, vec![stack_item])
            .await?;
        let address = response.stack[0].as_cell()?.parser().load_address()?;
        Ok(address)
    }

    async fn do_final_step(
        &self,
        address: &TonAddress,
        input: &str,
        step: &ValidationStep,
    ) -> Result<bool> {
        ensure!(
            step.args.len() == 1,
            "Invalid step args number, expected 1, got {}",
            step.args.len()
        );
        let stack_item = StackItem::from_input_and_step(input, step.args[0])?;
        let response = self
            .make_call(address, &step.method, vec![stack_item])
            .await?;
        let address = response.stack[0].as_num()?;

        let success = address == "-0x1";
        Ok(success)
    }

    async fn verify(&self, auth_contract_id: &str, args: VerifyArgs) -> Result<bool> {
        let treasury_address = TonAddress::from_base64_url(auth_contract_id)?;

        let validation_schema = {
            let Some(metadata) = args.metadata else {
                bail!("Metadata is required for stellar validation");
            };
            serde_json::from_str::<ValidationSchema>(&metadata)
                .context("Failed to parse Stellar Validation structure from metadata")?
        };

        let inputs: Vec<String> = serde_json::from_str(&args.user_payload)?;

        if inputs.len() != validation_schema.len() {
            bail!(
                "Invalid number of inputs: expected {}, got {}",
                validation_schema.len(),
                inputs.len()
            );
        }

        let input_and_step = inputs
            .iter()
            .zip(validation_schema.iter())
            .collect::<Vec<_>>();

        let is_verify_success = match input_and_step.split_last() {
            Some((last, rest)) => {
                let mut current_address = treasury_address;
                for (input, step) in rest {
                    let address = self
                        .do_intermediate_step(&current_address, input, step)
                        .await?;
                    current_address = address;
                }

                let (last_input, last_step) = last;
                self.do_final_step(&current_address, last_input, last_step)
                    .await?
            }
            None => bail!("Validation structure is empty"),
        };

        Ok(is_verify_success)
    }
}

#[async_trait]
impl SingleVerifier for TonSingleVerifier {
    fn get_endpoint(&self) -> String {
        self.server.clone()
    }
}

#[cfg(test)]
mod tests {
    
    use crate::ton::{StackItem, StepArgType, TonSingleVerifier, ValidationSchema, ValidationStep};
    use anyhow::Result;
    use serde_json::json;
    use std::sync::Arc;
    use tonlib_core::TonAddress;

    #[test]
    fn deserialize_validation_metadata() -> Result<()> {
        let data = json!(
            [
                {
                    "method": "get_deposit_jetton_address",
                    "args": ["int"],
                },
                {
                    "method": "verify_withdraw",
                    "args": ["slice"]
                }
            ]
        )
        .to_string();
        let _ = serde_json::from_str::<ValidationSchema>(&data)?;
        Ok(())
    }

    #[test]
    fn deserialize_validation_metadata_raw() -> Result<()> {
        let data = r#"[{"method": "get_deposit_jetton_address", "args": ["int"]}, {"method": "verify_withdraw", "args": ["slice"]}]"#;
        let _ = serde_json::from_str::<ValidationSchema>(data)?;
        Ok(())
    }

    #[tokio::test]
    async fn foo() -> Result<()> {
        let json = json!({
  "address": "EQANEViM3AKQzi6Aj3sEeyqFu8pXqhy9Q9xGoId_0qp3CNVJ",
  "method": "get_deposit_jetton_address",
  "stack": [
    ["num", "1753218716000000003679"]
  ],
  "seqno": 0
});
        let server = "https://rpc.ankr.com/premium-http/ton_api_v2/a916ff1ddacb18f87e2f6fcd51f641e6be9d112804003079de385f68a49b06d6".to_string(); // not ok
        // let server = "https://special-lively-hill.ton-mainnet.quiknode.pro/c82f675ec47224dce87479c0877f3c4014687307".to_string(); // not ok

        let client = Arc::new(reqwest::Client::new());

        let x = client
            .post(format!("{server}/runGetMethod"))
            .json(&json)
            .send()
            .await?
            .text()
            .await?;

        dbg!(&x);

        Ok(())
    }

    #[tokio::test]
    async fn test_ton_call() -> Result<()> {
        // let server = "https://toncenter.com/api/v3".to_string(); // ok
        // let server = "https://special-lively-hill.ton-mainnet.quiknode.pro/c82f675ec47224dce87479c0877f3c4014687307".to_string(); // not ok
        let server = "https://rpc.ankr.com/premium-http/ton_api_v3/a916ff1ddacb18f87e2f6fcd51f641e6be9d112804003079de385f68a49b06d6".to_string(); // not ok

        let client = Arc::new(reqwest::Client::new());
        let verifier = TonSingleVerifier::new(client, server);

        let addr_raw = "EQANEViM3AKQzi6Aj3sEeyqFu8pXqhy9Q9xGoId_0qp3CNVJ";
        let addr = TonAddress::from_base64_url(addr_raw)?;

        let nonce = "1753218716000000003679".to_string();
        let method = "get_deposit_jetton_address";

        let expected_addr_raw = "EQAgwUhaRZwU77BXUVEbtnEN8tplzDWMqUr0TbXWfez58tTL";
        let expected_addr = TonAddress::from_base64_url(expected_addr_raw)?;

        let stack = vec![StackItem::Num(nonce)];

        let response = verifier.make_call(&addr, method, stack).await?;

        let actual_addr = response.stack[0].as_cell()?.parser().load_address()?;

        assert_eq!(actual_addr, expected_addr);
        Ok(())
    }

    #[tokio::test]
    async fn test_intermediate_step() -> Result<()> {
        let server = "https://toncenter.com/api/v3".to_string();
        let client = Arc::new(reqwest::Client::new());
        let verifier = TonSingleVerifier::new(client, server);

        let addr_raw = "EQANEViM3AKQzi6Aj3sEeyqFu8pXqhy9Q9xGoId_0qp3CNVJ";
        let addr = TonAddress::from_base64_url(addr_raw)?;

        let expected_addr_raw = "EQAgwUhaRZwU77BXUVEbtnEN8tplzDWMqUr0TbXWfez58tTL";
        let expected_addr = TonAddress::from_base64_url(expected_addr_raw)?;

        let input = "1753218716000000003679";

        let actual_addr = verifier
            .do_intermediate_step(
                &addr,
                input,
                &ValidationStep {
                    method: "get_deposit_jetton_address".to_string(),
                    args: vec![StepArgType::Int],
                },
            )
            .await?;

        assert_eq!(actual_addr, expected_addr);

        Ok(())
    }

    #[tokio::test]
    async fn test_final_step() -> Result<()> {
        let server = "https://toncenter.com/api/v3".to_string();
        let client = Arc::new(reqwest::Client::new());
        let verifier = TonSingleVerifier::new(client, server);

        let addr_raw = "EQAgwUhaRZwU77BXUVEbtnEN8tplzDWMqUr0TbXWfez58tTL";
        let addr = TonAddress::from_base64_url(addr_raw)?;

        let proof = "bcb143828f64d7e4bf0b6a8e66a2a2d03c916c16e9e9034419ae778b9f699d3c";

        let success = verifier
            .do_final_step(
                &addr,
                proof,
                &ValidationStep {
                    method: "verify_withdraw".to_string(),
                    args: vec![StepArgType::Slice],
                },
            )
            .await?;

        assert!(success);

        Ok(())
    }

    #[tokio::test]
    async fn test_final_step_bad() -> Result<()> {
        let server = "https://toncenter.com/api/v3".to_string();
        let client = Arc::new(reqwest::Client::new());
        let verifier = TonSingleVerifier::new(client, server);

        let addr_raw = "EQAgwUhaRZwU77BXUVEbtnEN8tplzDWMqUr0TbXWfez58tTL";
        let addr = TonAddress::from_base64_url(addr_raw)?;

        let proof = "ccb143828f64d7e4bf0b6a8e66a2a2d03c916c16e9e9034419ae778b9f699d3c";

        let success = verifier
            .do_final_step(
                &addr,
                proof,
                &ValidationStep {
                    method: "verify_withdraw".to_string(),
                    args: vec![StepArgType::Slice],
                },
            )
            .await?;

        assert!(!success);

        Ok(())
    }
}
