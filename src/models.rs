use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Node chain summary used by the indexer.
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BlockchainInfo {
    pub chain: String,
    pub blocks: u64,
    pub headers: u64,
    pub bestblockhash: String,
    #[serde(default)]
    pub estimatedheight: Option<u64>,
    #[serde(default)]
    pub verificationprogress: Option<f64>,
    #[serde(default)]
    pub chain_supply: Option<ChainValue>,
    #[serde(default)]
    pub value_pools: Vec<ValuePool>,
}

/// Exact chain value as reported in atomic units.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChainValue {
    #[serde(default)]
    pub chain_value_zat: Option<i64>,
    #[serde(default)]
    pub monitored: Option<bool>,
}

/// One public value-pool snapshot.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ValuePool {
    pub id: String,
    #[serde(default)]
    pub chain_value_zat: Option<i64>,
    #[serde(default)]
    pub value_delta_zat: Option<i64>,
    #[serde(default)]
    pub monitored: Option<bool>,
}

/// Verbose Wcash block response.
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RpcBlock {
    pub hash: String,
    pub height: u64,
    pub confirmations: i64,
    pub size: u64,
    pub time: i64,
    pub bits: String,
    pub difficulty: Value,
    pub nonce: String,
    pub solution: String,
    pub merkleroot: String,
    #[serde(default)]
    pub blockcommitments: Option<String>,
    #[serde(default)]
    pub previousblockhash: Option<String>,
    #[serde(default)]
    pub nextblockhash: Option<String>,
    #[serde(default)]
    pub tx: Vec<RpcTransaction>,
    #[serde(default)]
    pub chain_supply: Option<ChainValue>,
    #[serde(default)]
    pub value_pools: Vec<ValuePool>,
    #[serde(flatten)]
    pub extra: serde_json::Map<String, Value>,
}

/// Verbose Wcash transaction response.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RpcTransaction {
    pub txid: String,
    #[serde(default)]
    pub authdigest: Option<String>,
    pub version: i64,
    pub size: u64,
    #[serde(default)]
    pub locktime: u64,
    #[serde(default)]
    pub expiryheight: u64,
    #[serde(default)]
    pub vin: Vec<RpcInput>,
    #[serde(default)]
    pub vout: Vec<RpcOutput>,
    #[serde(default, rename = "vShieldedSpend")]
    pub shielded_spends: Vec<Value>,
    #[serde(default, rename = "vShieldedOutput")]
    pub shielded_outputs: Vec<Value>,
    #[serde(default)]
    pub orchard: Option<Value>,
    #[serde(default)]
    pub ironwood: Option<Value>,
    #[serde(default)]
    pub value_balance_zat: Option<i64>,
    #[serde(default)]
    pub hex: Option<String>,
    #[serde(flatten)]
    pub extra: serde_json::Map<String, Value>,
}

/// One transparent input, or a coinbase marker.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct RpcInput {
    #[serde(default)]
    pub txid: Option<String>,
    #[serde(default)]
    pub vout: Option<u32>,
    #[serde(default)]
    pub coinbase: Option<String>,
    #[serde(default)]
    pub sequence: Option<u64>,
    #[serde(rename = "scriptSig", default)]
    pub script_sig: Option<Value>,
    #[serde(flatten)]
    pub extra: serde_json::Map<String, Value>,
}

/// One transparent output.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RpcOutput {
    pub value_zat: i64,
    pub n: u32,
    #[serde(rename = "scriptPubKey")]
    pub script_pub_key: ScriptPubKey,
    #[serde(flatten)]
    pub extra: serde_json::Map<String, Value>,
}

/// Decoded transparent output script.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ScriptPubKey {
    #[serde(default)]
    pub asm: Option<String>,
    #[serde(default)]
    pub hex: Option<String>,
    #[serde(rename = "type", default)]
    pub script_type: Option<String>,
    #[serde(default)]
    pub address: Option<String>,
    #[serde(default)]
    pub addresses: Vec<String>,
}

impl ScriptPubKey {
    /// Returns the single public address represented by this script when known.
    pub fn primary_address(&self) -> Option<&str> {
        self.address
            .as_deref()
            .or_else(|| (self.addresses.len() == 1).then(|| self.addresses[0].as_str()))
    }
}

/// Parent-chain block metadata when a hash is known to the configured node.
#[derive(Clone, Debug, Deserialize)]
pub struct ParentBlock {
    pub hash: String,
    pub height: u64,
    pub confirmations: i64,
    pub time: i64,
    pub bits: String,
    pub difficulty: Value,
    #[serde(default)]
    pub tx: Vec<Value>,
}

/// Result of exact Wcash AuxPoW verification and optional parent lookup.
#[derive(Clone, Debug)]
pub struct VerifiedAuxPow {
    pub witness_hash: String,
    pub proof_version: u8,
    pub proof_size: usize,
    pub parent_block_hash: String,
    pub parent_header_bits: String,
    pub parent_hash_meets_claimed_target: bool,
    pub parent_coinbase_txid: String,
    pub parent_merkle_depth: usize,
    pub parent_coinbase_index: u32,
    pub auth_data_merkle_depth: usize,
    pub auth_data_coinbase_index: u32,
    pub auxiliary_merkle_depth: usize,
    pub auxiliary_index: u32,
    pub verification_state: String,
    pub verifier_version: String,
    pub exact_witness_state: String,
    pub witness_confirmations: Option<u32>,
    pub parent_observations: Vec<ParentObservation>,
    pub parent_lookup_state: ParentLookupState,
    pub parent_sources_agree: bool,
}

/// One independent observation of the parent hash.
#[derive(Clone, Debug)]
pub struct ParentObservation {
    pub source: String,
    pub state: ParentLookupState,
    pub block: Option<ParentBlock>,
    pub embedded_header_matches: Option<bool>,
    pub checked_at: DateTime<Utc>,
}

/// Result of checking the exact parent hash against a canonical parent node.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ParentLookupState {
    Canonical,
    Orphaned,
    NotFound,
    Unavailable,
    NotConfigured,
    Disagreement,
}

impl ParentLookupState {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Canonical => "canonical",
            Self::Orphaned => "orphaned",
            Self::NotFound => "not_found",
            Self::Unavailable => "unavailable",
            Self::NotConfigured => "not_configured",
            Self::Disagreement => "disagreement",
        }
    }
}

/// Fully materialized block ready for one atomic database transaction.
#[derive(Clone, Debug)]
pub struct IndexedBlock {
    pub block: RpcBlock,
    pub auxpow: Option<VerifiedAuxPow>,
    pub fetched_at: DateTime<Utc>,
    pub raw_block: Vec<u8>,
    pub raw: Value,
}

/// Cursor-based list metadata exposed by the HTTP API.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PageMeta {
    pub next_cursor: Option<String>,
    pub indexed_height: Option<u64>,
    pub node_height: Option<u64>,
    pub freshness_seconds: Option<i64>,
    pub network: String,
}

/// Stable API envelope.
#[derive(Clone, Debug, Serialize)]
pub struct ApiEnvelope<T: Serialize> {
    pub data: T,
    pub meta: PageMeta,
}
