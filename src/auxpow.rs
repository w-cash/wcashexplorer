use chrono::Utc;
use futures::future::join_all;
use sha2::{Digest, Sha256};
use wcash_zcash_aux::{AuxPowProof, PARENT_HEADER_BYTES, PROOF_MAGIC, Target};

use crate::{
    error::{ExplorerError, Result},
    models::{ParentLookupState, ParentObservation, RpcBlock, VerifiedAuxPow},
    rpc::ZebraRpc,
};

const WITNESS_HASH_DOMAIN: &[u8] = b"WcashExplorer/AuxPoW-witness/v1\0";
const VERIFIER_VERSION: &str = "wcash-zcash-aux@72038cee";
const SOLUTION_OFFSET: usize = 140;

/// Performs local consensus verification plus exact Wcash and parent-chain lookups.
#[derive(Clone)]
pub struct AuxPowVerifier {
    wcash: ZebraRpc,
    parents: Vec<ZebraRpc>,
}

impl AuxPowVerifier {
    pub fn new(wcash: ZebraRpc, parents: Vec<ZebraRpc>) -> Self {
        Self { wcash, parents }
    }

    /// Returns `None` only for the genesis block, whose solution is native genesis work.
    pub async fn verify(
        &self,
        block: &RpcBlock,
        raw_block: &[u8],
    ) -> Result<Option<VerifiedAuxPow>> {
        let proof_bytes = decode_hex("Wcash solution", &block.solution, 256 * 1024)?;
        let raw_solution = solution_from_raw_block(raw_block)?;
        if raw_solution != proof_bytes {
            return Err(ExplorerError::InvalidAuxPow(
                "raw block witness does not match the verbose block solution".to_owned(),
            ));
        }
        if !proof_bytes.starts_with(&PROOF_MAGIC) {
            if block.height == 0 {
                return Ok(None);
            }
            return Err(ExplorerError::InvalidAuxPow(
                "a non-genesis Wcash block did not carry the WCAZ proof marker".to_owned(),
            ));
        }

        let proof = AuxPowProof::decode(&proof_bytes)
            .map_err(|error| ExplorerError::InvalidAuxPow(error.to_string()))?;
        let child_hash = parse_display_hash(&block.hash)?;
        let required_target = compact_target(&block.bits)?;
        let validated = proof
            .validate(child_hash, required_target)
            .map_err(|error| ExplorerError::InvalidAuxPow(error.to_string()))?;

        let parent_header = proof.parent_header();
        let parent_block_hash = display_hash(parent_header.block_hash().into_le_bytes());
        let parent_coinbase_txid = display_hash(validated.coinbase_transaction_id());
        let parent_header_bits = format!("{:08x}", parent_header.advertised_n_bits());
        let parent_hash_meets_claimed_target = compact_target(&parent_header_bits)
            .map(|target| target.is_met_by_le_hash(parent_header.block_hash().into_le_bytes()))
            .unwrap_or(false);

        let witness_hash = block_witness_hash(raw_block);
        let exact_status = self
            .wcash
            .aux_block_status(&block.hash, &block.solution)
            .await?;
        if !matches!(exact_status, crate::rpc::AuxStatus::BestChain { .. }) {
            return Err(ExplorerError::InvalidAuxPow(format!(
                "Wcash rejected the exact witness as canonical: {}",
                exact_status.state()
            )));
        }

        let observations = join_all(
            self.parents
                .iter()
                .map(|rpc| observe_parent(rpc, &parent_block_hash, parent_header.as_bytes())),
        )
        .await;
        let (parent_lookup_state, parent_sources_agree) = classify_observations(&observations);

        Ok(Some(VerifiedAuxPow {
            witness_hash,
            proof_version: proof_bytes[PROOF_MAGIC.len()],
            proof_size: proof_bytes.len(),
            parent_block_hash,
            parent_header_bits,
            parent_hash_meets_claimed_target,
            parent_coinbase_txid,
            parent_merkle_depth: proof.parent_merkle_branch().len(),
            parent_coinbase_index: proof.parent_coinbase_index(),
            auth_data_merkle_depth: proof.auth_data_merkle_branch().len(),
            auth_data_coinbase_index: proof.auth_data_coinbase_index(),
            auxiliary_merkle_depth: proof.auxiliary_merkle_branch().len(),
            auxiliary_index: proof.auxiliary_index(),
            verification_state: "auxpow_verified".to_owned(),
            verifier_version: VERIFIER_VERSION.to_owned(),
            exact_witness_state: exact_status.state().to_owned(),
            witness_confirmations: exact_status.confirmations(),
            parent_observations: observations,
            parent_lookup_state,
            parent_sources_agree,
        }))
    }
}

async fn observe_parent(
    rpc: &ZebraRpc,
    hash: &str,
    embedded_header: &[u8; PARENT_HEADER_BYTES],
) -> ParentObservation {
    let checked_at = Utc::now();
    let source = rpc.label().to_owned();
    let result = tokio::try_join!(rpc.parent_block(hash), rpc.block_raw(hash));
    match result {
        Ok((block, raw)) => {
            let header_matches = raw
                .get(..PARENT_HEADER_BYTES)
                .is_some_and(|header| header == embedded_header);
            let state = if !header_matches {
                ParentLookupState::Disagreement
            } else if block.confirmations < 0 {
                ParentLookupState::Orphaned
            } else {
                ParentLookupState::Canonical
            };
            ParentObservation {
                source,
                state,
                block: Some(block),
                embedded_header_matches: Some(header_matches),
                checked_at,
            }
        }
        Err(ExplorerError::Rpc { code, .. }) if code == -5 || code == -8 => ParentObservation {
            source,
            state: ParentLookupState::NotFound,
            block: None,
            embedded_header_matches: None,
            checked_at,
        },
        Err(_) => ParentObservation {
            source,
            state: ParentLookupState::Unavailable,
            block: None,
            embedded_header_matches: None,
            checked_at,
        },
    }
}

fn classify_observations(observations: &[ParentObservation]) -> (ParentLookupState, bool) {
    if observations.is_empty() {
        return (ParentLookupState::NotConfigured, false);
    }
    let first = observations[0].state;
    let sources_agree = observations.len() >= 2
        && observations
            .iter()
            .all(|observation| observation.state == first);
    let has = |state| {
        observations
            .iter()
            .any(|observation| observation.state == state)
    };
    let classified = if has(ParentLookupState::Disagreement)
        || (has(ParentLookupState::Canonical)
            && (has(ParentLookupState::NotFound) || has(ParentLookupState::Orphaned)))
    {
        ParentLookupState::Disagreement
    } else if has(ParentLookupState::Canonical) {
        ParentLookupState::Canonical
    } else if has(ParentLookupState::Orphaned) {
        ParentLookupState::Orphaned
    } else if has(ParentLookupState::NotFound) {
        ParentLookupState::NotFound
    } else {
        ParentLookupState::Unavailable
    };
    (classified, sources_agree)
}

/// Returns a domain-separated identity for every serialized block witness variant.
pub fn block_witness_hash(raw_block: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(WITNESS_HASH_DOMAIN);
    hasher.update(raw_block);
    hex::encode(hasher.finalize())
}

fn solution_from_raw_block(raw_block: &[u8]) -> Result<Vec<u8>> {
    let prefix = raw_block.get(SOLUTION_OFFSET..).ok_or_else(|| {
        ExplorerError::InvalidNodeResponse("raw block is shorter than its fixed header".to_owned())
    })?;
    let (length, encoded_length) = decode_compact_size(prefix)?;
    let length = usize::try_from(length).map_err(|_| {
        ExplorerError::InvalidNodeResponse("raw block solution length overflow".to_owned())
    })?;
    if length > 256 * 1024 {
        return Err(ExplorerError::InvalidNodeResponse(
            "raw block solution exceeded its size bound".to_owned(),
        ));
    }
    let start = SOLUTION_OFFSET.saturating_add(encoded_length);
    let end = start.checked_add(length).ok_or_else(|| {
        ExplorerError::InvalidNodeResponse("raw block solution range overflow".to_owned())
    })?;
    raw_block
        .get(start..end)
        .map(<[u8]>::to_vec)
        .ok_or_else(|| {
            ExplorerError::InvalidNodeResponse("raw block solution is truncated".to_owned())
        })
}

fn decode_compact_size(bytes: &[u8]) -> Result<(u64, usize)> {
    let marker = *bytes.first().ok_or_else(|| {
        ExplorerError::InvalidNodeResponse("raw block omitted its solution length".to_owned())
    })?;
    match marker {
        0x00..=0xfc => Ok((u64::from(marker), 1)),
        0xfd => {
            let encoded: [u8; 2] = bytes
                .get(1..3)
                .and_then(|value| value.try_into().ok())
                .ok_or_else(|| {
                    ExplorerError::InvalidNodeResponse(
                        "truncated compact solution length".to_owned(),
                    )
                })?;
            let value = u64::from(u16::from_le_bytes(encoded));
            if value < 0xfd {
                return Err(ExplorerError::InvalidNodeResponse(
                    "non-canonical compact solution length".to_owned(),
                ));
            }
            Ok((value, 3))
        }
        0xfe => {
            let encoded: [u8; 4] = bytes
                .get(1..5)
                .and_then(|value| value.try_into().ok())
                .ok_or_else(|| {
                    ExplorerError::InvalidNodeResponse(
                        "truncated compact solution length".to_owned(),
                    )
                })?;
            let value = u64::from(u32::from_le_bytes(encoded));
            if u16::try_from(value).is_ok() {
                return Err(ExplorerError::InvalidNodeResponse(
                    "non-canonical compact solution length".to_owned(),
                ));
            }
            Ok((value, 5))
        }
        0xff => {
            let encoded: [u8; 8] = bytes
                .get(1..9)
                .and_then(|value| value.try_into().ok())
                .ok_or_else(|| {
                    ExplorerError::InvalidNodeResponse(
                        "truncated compact solution length".to_owned(),
                    )
                })?;
            let value = u64::from_le_bytes(encoded);
            if u32::try_from(value).is_ok() {
                return Err(ExplorerError::InvalidNodeResponse(
                    "non-canonical compact solution length".to_owned(),
                ));
            }
            Ok((value, 9))
        }
    }
}

fn parse_display_hash(value: &str) -> Result<[u8; 32]> {
    let mut bytes = [0_u8; 32];
    hex::decode_to_slice(value, &mut bytes).map_err(|error| {
        ExplorerError::InvalidAuxPow(format!("invalid Wcash block hash: {error}"))
    })?;
    bytes.reverse();
    Ok(bytes)
}

fn display_hash(mut raw: [u8; 32]) -> String {
    raw.reverse();
    hex::encode(raw)
}

fn compact_target(bits: &str) -> Result<Target> {
    if bits.len() != 8 || !bits.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(ExplorerError::InvalidAuxPow(
            "difficulty bits must be exactly four hexadecimal bytes".to_owned(),
        ));
    }
    let compact = u32::from_str_radix(bits, 16).map_err(|error| {
        ExplorerError::InvalidAuxPow(format!("invalid difficulty bits: {error}"))
    })?;
    let size = (compact >> 24) as usize;
    let word = compact & 0x007f_ffff;
    if word == 0 || compact & 0x0080_0000 != 0 {
        return Err(ExplorerError::InvalidAuxPow(
            "difficulty target is zero or negative".to_owned(),
        ));
    }

    let mut target = [0_u8; 32];
    if size <= 3 {
        let value = word >> (8 * (3 - size));
        target[..4].copy_from_slice(&value.to_le_bytes());
    } else {
        let offset = size - 3;
        if offset + 3 > target.len() {
            return Err(ExplorerError::InvalidAuxPow(
                "difficulty target overflows 256 bits".to_owned(),
            ));
        }
        let word_bytes = word.to_le_bytes();
        target[offset..offset + 3].copy_from_slice(&word_bytes[..3]);
    }
    Target::from_le_bytes(target).map_err(|error| ExplorerError::InvalidAuxPow(error.to_string()))
}

fn decode_hex(label: &str, value: &str, maximum_bytes: usize) -> Result<Vec<u8>> {
    if !value.len().is_multiple_of(2) || value.len() / 2 > maximum_bytes {
        return Err(ExplorerError::InvalidAuxPow(format!(
            "{label} has an invalid encoded length"
        )));
    }
    hex::decode(value)
        .map_err(|error| ExplorerError::InvalidAuxPow(format!("{label} is not hex: {error}")))
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use axum::{Json, Router, routing::post};
    use serde_json::{Value, json};

    use super::*;
    use crate::config::{RpcAuth, RpcConfig};

    #[test]
    fn decodes_wcash_and_bitcoin_style_targets() {
        let wcash = compact_target("1e008859").expect("valid Wcash target");
        let bitcoin = compact_target("1d00ffff").expect("valid Bitcoin target");
        assert_ne!(wcash.to_le_bytes(), [0; 32]);
        assert_ne!(bitcoin.to_le_bytes(), [0; 32]);
        assert!(compact_target("1d80ffff").is_err());
        assert!(compact_target("00000000").is_err());
    }

    #[test]
    fn parent_source_disagreement_is_not_hidden() {
        let now = Utc::now();
        let observations = vec![
            ParentObservation {
                source: "a".to_owned(),
                state: ParentLookupState::Canonical,
                block: None,
                embedded_header_matches: Some(true),
                checked_at: now,
            },
            ParentObservation {
                source: "b".to_owned(),
                state: ParentLookupState::NotFound,
                block: None,
                embedded_header_matches: None,
                checked_at: now,
            },
        ];
        assert_eq!(
            classify_observations(&observations),
            (ParentLookupState::Disagreement, false)
        );
    }

    #[test]
    fn no_parent_sources_is_explicitly_not_configured() {
        assert_eq!(
            classify_observations(&[]),
            (ParentLookupState::NotConfigured, false)
        );
    }

    #[tokio::test]
    async fn verifies_the_complete_auxpow_witness_without_a_parent_rpc() {
        async fn wcash_status(Json(request): Json<Value>) -> Json<Value> {
            assert_eq!(request["jsonrpc"], "2.0");
            assert_eq!(request["method"], "getauxblockstatus");
            assert_eq!(
                request["params"][0],
                "79cdcea38a54f99a59adddf124490a65de499e5677e693caa14dcf24b5f96007"
            );
            Json(json!({
                "jsonrpc": "2.0",
                "id": request["id"],
                "result": {"state": "best_chain", "confirmations": 48},
                "error": null
            }))
        }

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind loopback RPC fixture");
        let address = listener.local_addr().expect("fixture RPC address");
        let server = tokio::spawn(async move {
            axum::serve(listener, Router::new().route("/", post(wcash_status)))
                .await
                .expect("serve loopback RPC fixture");
        });

        let raw_block =
            hex::decode(include_str!("../tests/fixtures/wcash-testnet-block-1.hex").trim())
                .expect("valid raw block fixture");
        let solution = include_str!("../tests/fixtures/wcash-testnet-block-1-solution.hex")
            .trim()
            .to_owned();
        let block: RpcBlock = serde_json::from_value(json!({
            "hash": "79cdcea38a54f99a59adddf124490a65de499e5677e693caa14dcf24b5f96007",
            "height": 1,
            "confirmations": 48,
            "size": raw_block.len(),
            "time": 1_788_812_824_i64,
            "bits": "1e008859",
            "difficulty": 1.0,
            "nonce": "00".repeat(32),
            "solution": solution,
            "merkleroot": "00".repeat(32)
        }))
        .expect("valid verbose block fixture");
        let wcash = ZebraRpc::new(
            RpcConfig {
                label: "wcash-fixture".to_owned(),
                url: format!("http://{address}/")
                    .parse()
                    .expect("valid fixture RPC URL"),
                auth: RpcAuth::None,
            },
            Duration::from_secs(2),
        )
        .expect("valid fixture RPC client");

        let verified = AuxPowVerifier::new(wcash, Vec::new())
            .verify(&block, &raw_block)
            .await
            .expect("valid Wcash-only AuxPoW verification")
            .expect("non-genesis proof");
        assert_eq!(verified.verification_state, "auxpow_verified");
        assert_eq!(verified.exact_witness_state, "best_chain");
        assert_eq!(verified.witness_confirmations, Some(48));
        assert_eq!(
            verified.parent_lookup_state,
            ParentLookupState::NotConfigured
        );
        assert!(!verified.parent_sources_agree);
        assert!(verified.parent_observations.is_empty());

        server.abort();
        let _ = server.await;
    }

    #[test]
    fn compact_solution_lengths_are_canonical() {
        assert_eq!(
            decode_compact_size(&[0xfc]).expect("single-byte length"),
            (252, 1)
        );
        assert_eq!(
            decode_compact_size(&[0xfd, 0xfd, 0x00]).expect("u16 length"),
            (253, 3)
        );
        assert!(decode_compact_size(&[0xfd, 0xfc, 0x00]).is_err());
        assert!(decode_compact_size(&[0xfe, 0xff, 0xff, 0x00, 0x00]).is_err());
    }

    #[test]
    fn validates_the_published_wcash_testnet_block_one_fixture() {
        let raw = hex::decode(include_str!("../tests/fixtures/wcash-testnet-block-1.hex").trim())
            .expect("valid raw block fixture");
        let expected_solution = hex::decode(
            include_str!("../tests/fixtures/wcash-testnet-block-1-solution.hex").trim(),
        )
        .expect("valid solution fixture");
        let solution = solution_from_raw_block(&raw).expect("solution in raw block");
        assert_eq!(solution, expected_solution);
        assert_eq!(solution.len(), 1_803);

        let proof = AuxPowProof::decode(&solution).expect("valid AuxPoW encoding");
        let child_hash =
            parse_display_hash("79cdcea38a54f99a59adddf124490a65de499e5677e693caa14dcf24b5f96007")
                .expect("valid child hash");
        let validated = proof
            .validate(
                child_hash,
                compact_target("1e008859").expect("valid target"),
            )
            .expect("fixture must pass the pinned consensus verifier");

        assert_eq!(
            display_hash(proof.parent_header().block_hash().into_le_bytes()),
            "0000005d48352ca15798f834f835a9b68e14bda9053c98251124725f8b32ad52"
        );
        assert_eq!(
            display_hash(validated.coinbase_transaction_id()),
            "1a4466dbf0b2e24e7094b8f6a8b372758b1b37249e99a78195f517149e3aeb4b"
        );
    }

    #[test]
    fn detects_raw_and_verbose_witness_mismatch() {
        let mut raw =
            hex::decode(include_str!("../tests/fixtures/wcash-testnet-block-1.hex").trim())
                .expect("valid raw block fixture");
        let expected_solution = solution_from_raw_block(&raw).expect("solution in raw block");
        raw[SOLUTION_OFFSET + 8] ^= 1;
        let changed_solution = solution_from_raw_block(&raw).expect("changed solution decodes");
        assert_ne!(changed_solution, expected_solution);
    }
}
