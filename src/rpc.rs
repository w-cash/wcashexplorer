use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};

use reqwest::{Client, RequestBuilder};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use serde_json::{Value, json};

use crate::{
    config::{RpcAuth, RpcConfig},
    error::{ExplorerError, Result},
    models::{BlockchainInfo, ParentBlock, RpcBlock},
};

const MAX_RPC_RESPONSE_BYTES: usize = 32 * 1024 * 1024;

/// Strict, authenticated client for a private Zebra JSON-RPC endpoint.
#[derive(Clone)]
pub struct ZebraRpc {
    config: RpcConfig,
    client: Client,
    next_id: Arc<AtomicU64>,
}

impl ZebraRpc {
    /// Creates a client with bounded request duration and no ambient proxy use.
    pub fn new(config: RpcConfig, timeout: std::time::Duration) -> Result<Self> {
        let client = Client::builder()
            .timeout(timeout)
            .no_proxy()
            .https_only(false)
            .build()?;
        Ok(Self {
            config,
            client,
            next_id: Arc::new(AtomicU64::new(1)),
        })
    }

    /// Stable non-secret source name used in parent observations.
    pub fn label(&self) -> &str {
        &self.config.label
    }

    /// Confirms that an RPC endpoint belongs to the expected chain identity.
    pub async fn validate_chain_identity(
        &self,
        expected_chain: &str,
        expected_genesis: &str,
    ) -> Result<()> {
        let info = self.blockchain_info().await?;
        let genesis = self.block_hash(0).await?;
        if info.chain != expected_chain || genesis != expected_genesis {
            return Err(ExplorerError::Config(format!(
                "RPC {} chain identity mismatch: expected {expected_chain}/{expected_genesis}, received {}/{}",
                self.label(),
                info.chain,
                genesis
            )));
        }
        Ok(())
    }

    /// Returns current node network and tip metadata.
    pub async fn blockchain_info(&self) -> Result<BlockchainInfo> {
        self.call("getblockchaininfo", json!([])).await
    }

    /// Returns the canonical block hash at `height`.
    pub async fn block_hash(&self, height: u64) -> Result<String> {
        self.call("getblockhash", json!([height])).await
    }

    /// Fetches a block with fully decoded transactions and preserves its raw JSON.
    pub async fn block_verbose(&self, hash: &str) -> Result<(RpcBlock, Value)> {
        validate_hash(hash)?;
        let value: Value = self.call("getblock", json!([hash, 2])).await?;
        let block = serde_json::from_value(value.clone()).map_err(|error| {
            ExplorerError::InvalidNodeResponse(format!("invalid verbose block: {error}"))
        })?;
        Ok((block, value))
    }

    /// Fetches canonical serialized block bytes.
    pub async fn block_raw(&self, hash: &str) -> Result<Vec<u8>> {
        validate_hash(hash)?;
        let encoded: String = self.call("getblock", json!([hash, 0])).await?;
        decode_hex_bounded("raw block", &encoded, MAX_RPC_RESPONSE_BYTES / 2)
    }

    /// Asks Wcash for the exact witness-bound chain state.
    pub async fn aux_block_status(&self, block_hash: &str, proof_hex: &str) -> Result<AuxStatus> {
        validate_hash(block_hash)?;
        if proof_hex.len() > 512 * 1024 || !proof_hex.len().is_multiple_of(2) {
            return Err(ExplorerError::InvalidRequest(
                "AuxPoW proof has an invalid encoded length".to_owned(),
            ));
        }
        self.call("getauxblockstatus", json!([block_hash, proof_hex]))
            .await
    }

    /// Looks up an exact parent block hash on this node.
    pub async fn parent_block(&self, hash: &str) -> Result<ParentBlock> {
        validate_hash(hash)?;
        self.call("getblock", json!([hash, 1])).await
    }

    /// Executes a typed JSON-RPC method.
    pub async fn call<T>(&self, method: &str, params: Value) -> Result<T>
    where
        T: DeserializeOwned,
    {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let request = RpcRequest {
            jsonrpc: "2.0",
            id,
            method,
            params,
        };
        let builder = self.client.post(self.config.url.clone()).json(&request);
        let mut response = self.with_auth(builder).await?.send().await?;
        if !response.status().is_success() {
            return Err(ExplorerError::InvalidNodeResponse(format!(
                "node returned HTTP {}",
                response.status()
            )));
        }
        if response
            .content_length()
            .is_some_and(|length| length > MAX_RPC_RESPONSE_BYTES as u64)
        {
            return Err(ExplorerError::InvalidNodeResponse(
                "node response exceeded the configured size limit".to_owned(),
            ));
        }
        let mut bytes = Vec::with_capacity(
            response
                .content_length()
                .and_then(|length| usize::try_from(length).ok())
                .unwrap_or(8 * 1024)
                .min(MAX_RPC_RESPONSE_BYTES),
        );
        while let Some(chunk) = response.chunk().await? {
            if bytes.len().saturating_add(chunk.len()) > MAX_RPC_RESPONSE_BYTES {
                return Err(ExplorerError::InvalidNodeResponse(
                    "node response exceeded the configured size limit".to_owned(),
                ));
            }
            bytes.extend_from_slice(&chunk);
        }
        let envelope: RpcResponse<T> = serde_json::from_slice(&bytes).map_err(|error| {
            ExplorerError::InvalidNodeResponse(format!("invalid JSON-RPC response: {error}"))
        })?;
        ensure_json_rpc_version(&envelope.jsonrpc)?;
        if envelope.id != id {
            return Err(ExplorerError::InvalidNodeResponse(
                "JSON-RPC response ID did not match the request".to_owned(),
            ));
        }
        if let Some(error) = envelope.error {
            return Err(ExplorerError::Rpc {
                code: error.code,
                message: error.message,
            });
        }
        envelope.result.ok_or_else(|| {
            ExplorerError::InvalidNodeResponse("JSON-RPC response omitted result".to_owned())
        })
    }

    async fn with_auth(&self, builder: RequestBuilder) -> Result<RequestBuilder> {
        match &self.config.auth {
            RpcAuth::None => Ok(builder),
            RpcAuth::Basic { username, password } => {
                Ok(builder.basic_auth(username, Some(password)))
            }
            RpcAuth::Cookie(path) => {
                let cookie = tokio::fs::read_to_string(path).await.map_err(|error| {
                    ExplorerError::Config(format!(
                        "could not read RPC cookie {}: {error}",
                        path.display()
                    ))
                })?;
                let (username, password) = cookie.trim().split_once(':').ok_or_else(|| {
                    ExplorerError::Config(format!(
                        "RPC cookie {} has an invalid format",
                        path.display()
                    ))
                })?;
                Ok(builder.basic_auth(username, Some(password)))
            }
        }
    }
}

/// Exact-witness Wcash state returned by `getauxblockstatus`.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(tag = "state", rename_all = "snake_case", deny_unknown_fields)]
pub enum AuxStatus {
    BestChain { confirmations: u32 },
    SideChain,
    ConflictingWitness,
    Pending,
    Unknown,
}

impl AuxStatus {
    pub const fn state(&self) -> &'static str {
        match self {
            Self::BestChain { .. } => "best_chain",
            Self::SideChain => "side_chain",
            Self::ConflictingWitness => "conflicting_witness",
            Self::Pending => "pending",
            Self::Unknown => "unknown",
        }
    }

    pub const fn confirmations(&self) -> Option<u32> {
        match self {
            Self::BestChain { confirmations } => Some(*confirmations),
            _ => None,
        }
    }
}

#[derive(Debug, Serialize)]
struct RpcRequest<'a> {
    jsonrpc: &'static str,
    id: u64,
    method: &'a str,
    params: Value,
}

#[derive(Debug, Deserialize)]
struct RpcResponse<T> {
    jsonrpc: String,
    id: u64,
    result: Option<T>,
    error: Option<RpcError>,
}

#[derive(Debug, Deserialize)]
struct RpcError {
    code: i64,
    message: String,
}

fn validate_hash(value: &str) -> Result<()> {
    if value.len() != 64 || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(ExplorerError::InvalidRequest(
            "block hash must be exactly 32 bytes of hexadecimal".to_owned(),
        ));
    }
    Ok(())
}

fn ensure_json_rpc_version(value: &str) -> Result<()> {
    if value != "2.0" {
        return Err(ExplorerError::InvalidNodeResponse(
            "node response is not JSON-RPC 2.0".to_owned(),
        ));
    }
    Ok(())
}

fn decode_hex_bounded(label: &str, value: &str, maximum_bytes: usize) -> Result<Vec<u8>> {
    if !value.len().is_multiple_of(2) || value.len() / 2 > maximum_bytes {
        return Err(ExplorerError::InvalidNodeResponse(format!(
            "{label} has an invalid encoded length"
        )));
    }
    hex::decode(value).map_err(|error| {
        ExplorerError::InvalidNodeResponse(format!("{label} is not hexadecimal: {error}"))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_non_hash_identifiers() {
        assert!(validate_hash(&"ab".repeat(32)).is_ok());
        assert!(validate_hash("../cookie").is_err());
        assert!(validate_hash(&"00".repeat(33)).is_err());
    }

    #[test]
    fn aux_status_is_strict() {
        let parsed: AuxStatus = serde_json::from_value(json!({
            "state": "best_chain",
            "confirmations": 7
        }))
        .expect("valid response");
        assert_eq!(parsed.state(), "best_chain");
        assert_eq!(parsed.confirmations(), Some(7));
        assert!(serde_json::from_value::<AuxStatus>(json!({"state": "accepted"})).is_err());
    }

    #[test]
    fn rpc_responses_require_the_json_rpc_2_marker() {
        assert!(
            serde_json::from_value::<RpcResponse<Value>>(json!({
                "id": 1,
                "result": true,
                "error": null
            }))
            .is_err()
        );
        let wrong_version: RpcResponse<Value> = serde_json::from_value(json!({
            "jsonrpc": "1.0",
            "id": 1,
            "result": true,
            "error": null
        }))
        .expect("well-formed envelope with a wrong protocol version");
        assert!(ensure_json_rpc_version(&wrong_version.jsonrpc).is_err());
        assert!(ensure_json_rpc_version("2.0").is_ok());
    }
}
