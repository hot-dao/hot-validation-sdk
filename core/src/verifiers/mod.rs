use reqwest::Url;

pub mod evm;
pub mod near;
pub mod solana;
pub mod stellar;
pub mod ton;

/// An identification of the verifier (rpc endpoint). Used only for logging.
pub(crate) trait VerifierTag: Send + Sync + 'static {
    fn get_endpoint(&self) -> String; // TODO: Can we return a reference here?

    fn sanitized_endpoint(&self) -> String {
        let endpoint = self.get_endpoint();
        Url::parse(&endpoint)
            .map(|e| e.host().map(|h| h.to_string()).unwrap_or_default())
            .unwrap_or_default()
    }
}
