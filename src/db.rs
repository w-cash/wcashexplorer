use std::time::Duration;

use chrono::{DateTime, Utc};
use serde_json::Value;
use sha2::{Digest, Sha256};
use sqlx::{PgPool, Postgres, Row, Transaction, pool::PoolConnection, postgres::PgPoolOptions};
use uuid::Uuid;

use crate::{
    config::NetworkConfig,
    error::{ExplorerError, Result},
    models::{IndexedBlock, RpcTransaction},
};

const WRITER_LOCK_ID: i64 = 0x5743_4153_4858_504c;
const RAW_TX_HASH_DOMAIN: &[u8] = b"WcashExplorer/raw-transaction/v1\0";

/// PostgreSQL-backed explorer store.
#[derive(Clone)]
pub struct Database {
    pool: PgPool,
}

/// Canonical chain tip persisted by the indexer.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ChainTip {
    pub height: u64,
    pub block_hash: String,
    pub witness_hash: String,
}

impl Database {
    /// Connects with safe pool and statement timeout defaults.
    pub async fn connect(url: &str) -> Result<Self> {
        let pool = PgPoolOptions::new()
            .max_connections(16)
            .min_connections(1)
            .acquire_timeout(Duration::from_secs(10))
            .after_connect(|connection, _| {
                Box::pin(async move {
                    sqlx::query("SET statement_timeout = '15s'")
                        .execute(&mut *connection)
                        .await?;
                    sqlx::query("SET idle_in_transaction_session_timeout = '30s'")
                        .execute(&mut *connection)
                        .await?;
                    Ok(())
                })
            })
            .connect(url)
            .await?;
        Ok(Self { pool })
    }

    pub fn pool(&self) -> &PgPool {
        &self.pool
    }

    /// Applies embedded, checksummed migrations.
    pub async fn migrate(&self) -> Result<()> {
        sqlx::migrate!().run(&self.pool).await?;
        Ok(())
    }

    /// Binds this database permanently to one configured chain identity.
    pub async fn initialize_network(&self, network: &NetworkConfig) -> Result<()> {
        sqlx::query(
            "INSERT INTO networks (
                network_id, display_name, symbol, decimals, coinbase_maturity,
                target_spacing_seconds, max_supply_zat, initial_subsidy_zat,
                halving_interval, first_halving_height, genesis_hash
             ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)
             ON CONFLICT (network_id) DO NOTHING",
        )
        .bind(&network.id)
        .bind(&network.display_name)
        .bind(&network.symbol)
        .bind(i16::from(network.decimals))
        .bind(i64::from(network.coinbase_maturity))
        .bind(i64::from(network.target_spacing_seconds))
        .bind(network.max_supply_zat)
        .bind(network.initial_subsidy_zat)
        .bind(to_i64(network.halving_interval, "halving interval")?)
        .bind(to_i64(
            network.first_halving_height,
            "first halving height",
        )?)
        .bind(&network.genesis_hash)
        .execute(&self.pool)
        .await?;

        let row = sqlx::query(
            "SELECT display_name, symbol, decimals, coinbase_maturity,
                    target_spacing_seconds, max_supply_zat, initial_subsidy_zat,
                    halving_interval, first_halving_height, genesis_hash
             FROM networks WHERE network_id = $1",
        )
        .bind(&network.id)
        .fetch_one(&self.pool)
        .await?;
        let stored_genesis: String = row.try_get("genesis_hash")?;
        let stored_decimals: i16 = row.try_get("decimals")?;
        let stored_symbol: String = row.try_get("symbol")?;
        let stored_display_name: String = row.try_get("display_name")?;
        let stored_maturity: i64 = row.try_get("coinbase_maturity")?;
        let stored_spacing: i64 = row.try_get("target_spacing_seconds")?;
        let stored_max_supply: i64 = row.try_get("max_supply_zat")?;
        let stored_initial_subsidy: i64 = row.try_get("initial_subsidy_zat")?;
        let stored_halving_interval: i64 = row.try_get("halving_interval")?;
        let stored_first_halving: i64 = row.try_get("first_halving_height")?;
        if stored_genesis != network.genesis_hash
            || stored_decimals != i16::from(network.decimals)
            || stored_symbol != network.symbol
            || stored_display_name != network.display_name
            || stored_maturity != i64::from(network.coinbase_maturity)
            || stored_spacing != i64::from(network.target_spacing_seconds)
            || stored_max_supply != network.max_supply_zat
            || stored_initial_subsidy != network.initial_subsidy_zat
            || stored_halving_interval != to_i64(network.halving_interval, "halving interval")?
            || stored_first_halving != to_i64(network.first_halving_height, "first halving height")?
        {
            return Err(ExplorerError::Config(format!(
                "database network identity for {} does not match runtime configuration",
                network.id
            )));
        }
        sqlx::query(
            "INSERT INTO chain_state (network_id, status) VALUES ($1, 'starting')
             ON CONFLICT (network_id) DO NOTHING",
        )
        .bind(&network.id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Acquires the singleton indexer writer lease for this PostgreSQL database.
    pub async fn acquire_writer_lock(&self) -> Result<PoolConnection<Postgres>> {
        let mut connection = self.pool.acquire().await?;
        let acquired: bool = sqlx::query_scalar("SELECT pg_try_advisory_lock($1)")
            .bind(WRITER_LOCK_ID)
            .fetch_one(&mut *connection)
            .await?;
        if !acquired {
            return Err(ExplorerError::NotReady(
                "another explorer indexer already owns the writer lease".to_owned(),
            ));
        }
        Ok(connection)
    }

    /// Confirms that the current session still owns an advisory writer lock.
    pub async fn writer_lock_is_held(connection: &mut PoolConnection<Postgres>) -> Result<bool> {
        Ok(sqlx::query_scalar(
            "SELECT EXISTS(
                SELECT 1 FROM pg_locks
                WHERE pid = pg_backend_pid() AND locktype = 'advisory' AND granted
            )",
        )
        .fetch_one(&mut **connection)
        .await?)
    }

    /// Returns the currently selected canonical tip.
    pub async fn canonical_tip(&self, network_id: &str) -> Result<Option<ChainTip>> {
        let row = sqlx::query(
            "SELECT height, block_hash, witness_hash
             FROM canonical_chain
             WHERE network_id = $1
             ORDER BY height DESC
             LIMIT 1",
        )
        .bind(network_id)
        .fetch_optional(&self.pool)
        .await?;
        row.map(|row| {
            Ok(ChainTip {
                height: u64::try_from(row.try_get::<i64, _>("height")?).map_err(|_| {
                    ExplorerError::InvalidNodeResponse("negative stored height".to_owned())
                })?,
                block_hash: row.try_get("block_hash")?,
                witness_hash: row.try_get("witness_hash")?,
            })
        })
        .transpose()
    }

    /// Returns the selected hash at a canonical height.
    pub async fn canonical_hash(&self, network_id: &str, height: u64) -> Result<Option<String>> {
        let height = to_i64(height, "height")?;
        Ok(sqlx::query_scalar(
            "SELECT block_hash FROM canonical_chain WHERE network_id = $1 AND height = $2",
        )
        .bind(network_id)
        .bind(height)
        .fetch_optional(&self.pool)
        .await?)
    }

    /// Selects the stalest recent AuxPoW record for periodic evidence refresh.
    pub async fn evidence_refresh_candidate(
        &self,
        network_id: &str,
        depth: u64,
    ) -> Result<Option<(u64, String)>> {
        let row = sqlx::query(
            "WITH tip AS (
                SELECT MAX(height) AS height FROM canonical_chain WHERE network_id = $1
             )
             SELECT c.height, c.block_hash
             FROM canonical_chain c
             JOIN auxpow_links a ON a.block_hash = c.block_hash AND a.witness_hash = c.witness_hash
             CROSS JOIN tip
             WHERE c.network_id = $1
               AND c.height >= GREATEST(1, tip.height - $2)
               AND a.verified_at < now() - interval '30 seconds'
             ORDER BY a.verified_at ASC
             LIMIT 1",
        )
        .bind(network_id)
        .bind(to_i64(depth, "evidence refresh depth")?)
        .fetch_optional(&self.pool)
        .await?;
        row.map(|row| {
            Ok((
                u64::try_from(row.try_get::<i64, _>("height")?).map_err(|_| {
                    ExplorerError::InvalidNodeResponse(
                        "negative refresh candidate height".to_owned(),
                    )
                })?,
                row.try_get("block_hash")?,
            ))
        })
        .transpose()
    }

    /// Atomically persists one immutable block/witness and selects it canonically.
    pub async fn commit_block(&self, network_id: &str, indexed: &IndexedBlock) -> Result<()> {
        let mut transaction = self.pool.begin().await?;
        insert_block(&mut transaction, network_id, indexed).await?;
        select_canonical(&mut transaction, network_id, indexed).await?;
        transaction.commit().await?;
        Ok(())
    }

    /// Refreshes mutable witness observations without changing canonical selection.
    pub async fn refresh_block_evidence(
        &self,
        network_id: &str,
        indexed: &IndexedBlock,
    ) -> Result<()> {
        let mut transaction = self.pool.begin().await?;
        insert_block(&mut transaction, network_id, indexed).await?;
        transaction.commit().await?;
        Ok(())
    }

    /// Publishes a fully staged replacement branch in one database transaction.
    pub async fn replace_canonical_branch(
        &self,
        network_id: &str,
        old_tip: &ChainTip,
        ancestor: Option<(u64, String)>,
        replacement: &[IndexedBlock],
    ) -> Result<()> {
        let mut transaction = self.pool.begin().await?;
        let locked_tip = sqlx::query(
            "SELECT indexed_height, indexed_hash
             FROM chain_state WHERE network_id = $1 FOR UPDATE",
        )
        .bind(network_id)
        .fetch_one(&mut *transaction)
        .await?;
        let locked_height: Option<i64> = locked_tip.try_get("indexed_height")?;
        let locked_hash: Option<String> = locked_tip.try_get("indexed_hash")?;
        if locked_height != Some(to_i64(old_tip.height, "old tip height")?)
            || locked_hash.as_deref() != Some(old_tip.block_hash.as_str())
        {
            return Err(ExplorerError::NotReady(
                "canonical tip changed before the reorganization could be published".to_owned(),
            ));
        }

        validate_replacement_branch(ancestor.as_ref(), replacement)?;
        for indexed in replacement {
            insert_block(&mut transaction, network_id, indexed).await?;
        }

        let event_id = Uuid::new_v4();
        let ancestor_height = ancestor
            .as_ref()
            .map(|(height, _)| to_i64(*height, "ancestor height"))
            .transpose()?;
        let ancestor_hash = ancestor.as_ref().map(|(_, hash)| hash.as_str());
        sqlx::query(
            "INSERT INTO reorg_events (
                event_id, network_id, old_tip_height, old_tip_hash,
                common_ancestor_height, common_ancestor_hash, detected_at
             ) VALUES ($1, $2, $3, $4, $5, $6, now())",
        )
        .bind(event_id)
        .bind(network_id)
        .bind(to_i64(old_tip.height, "old tip height")?)
        .bind(&old_tip.block_hash)
        .bind(ancestor_height)
        .bind(ancestor_hash)
        .execute(&mut *transaction)
        .await?;

        match ancestor_height {
            Some(height) => {
                sqlx::query("DELETE FROM canonical_chain WHERE network_id = $1 AND height > $2")
                    .bind(network_id)
                    .bind(height)
                    .execute(&mut *transaction)
                    .await?;
            }
            None => {
                sqlx::query("DELETE FROM canonical_chain WHERE network_id = $1")
                    .bind(network_id)
                    .execute(&mut *transaction)
                    .await?;
            }
        }

        for indexed in replacement {
            insert_canonical_selector(&mut transaction, network_id, indexed).await?;
        }

        let new_tip = replacement.last();
        let indexed_height = new_tip
            .map(|indexed| to_i64(indexed.block.height, "replacement tip height"))
            .transpose()?
            .or(ancestor_height);
        let indexed_hash = new_tip
            .map(|indexed| indexed.block.hash.as_str())
            .or(ancestor_hash);
        let indexed_witness_hash = new_tip.map(witness_hash);
        sqlx::query(
            "UPDATE chain_state SET
                indexed_height = $2,
                indexed_hash = $3,
                indexed_witness_hash = $4,
                status = 'syncing',
                last_error = NULL,
                updated_at = now()
             WHERE network_id = $1",
        )
        .bind(network_id)
        .bind(indexed_height)
        .bind(indexed_hash)
        .bind(indexed_witness_hash)
        .execute(&mut *transaction)
        .await?;
        sqlx::query("UPDATE reorg_events SET completed_at = now() WHERE event_id = $1")
            .bind(event_id)
            .execute(&mut *transaction)
            .await?;
        transaction.commit().await?;
        Ok(())
    }

    /// Persists an indexer failure so readiness cannot outlive its worker.
    pub async fn mark_indexer_error(&self, network_id: &str, error: &str) -> Result<()> {
        sqlx::query(
            "UPDATE chain_state SET status = 'error', last_error = $2, updated_at = now()
             WHERE network_id = $1",
        )
        .bind(network_id)
        .bind(truncate(error, 1_000))
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Stores the latest observed node tip and indexer state.
    pub async fn observe_node_tip(
        &self,
        network_id: &str,
        node_height: u64,
        node_hash: &str,
    ) -> Result<()> {
        sqlx::query(
            "UPDATE chain_state SET
                node_height = $2,
                node_hash = $3,
                status = CASE
                    WHEN indexed_height = $2 AND indexed_hash = $3 THEN status
                    ELSE 'syncing'
                END,
                updated_at = now()
             WHERE network_id = $1",
        )
        .bind(network_id)
        .bind(to_i64(node_height, "node height")?)
        .bind(node_hash)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Stores the latest observed node tip and sets the completed indexer state.
    pub async fn update_node_tip(
        &self,
        network_id: &str,
        node_height: u64,
        node_hash: &str,
        status: &str,
        error: Option<&str>,
    ) -> Result<()> {
        sqlx::query(
            "UPDATE chain_state SET
                node_height = $2,
                node_hash = $3,
                status = $4,
                last_error = $5,
                updated_at = now()
             WHERE network_id = $1",
        )
        .bind(network_id)
        .bind(to_i64(node_height, "node height")?)
        .bind(node_hash)
        .bind(status)
        .bind(error.map(|value| truncate(value, 1_000)))
        .execute(&self.pool)
        .await?;
        Ok(())
    }
}

async fn insert_block(
    transaction: &mut Transaction<'_, Postgres>,
    network_id: &str,
    indexed: &IndexedBlock,
) -> Result<()> {
    let block = &indexed.block;
    let witness_hash = indexed.auxpow.as_ref().map_or_else(
        || crate::auxpow::block_witness_hash(&indexed.raw_block),
        |aux| aux.witness_hash.clone(),
    );
    let block_time = DateTime::<Utc>::from_timestamp(block.time, 0).ok_or_else(|| {
        ExplorerError::InvalidNodeResponse("block timestamp is out of range".to_owned())
    })?;
    let difficulty_text = scalar_text(&block.difficulty);
    sqlx::query(
        "INSERT INTO blocks (
            block_hash, network_id, height, previous_block_hash, merkle_root,
            block_commitments, block_time, size_bytes, transaction_count, bits,
            difficulty_text, nonce, chain_supply_zat, first_seen_at, raw_rpc
         ) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15)
         ON CONFLICT (block_hash) DO NOTHING",
    )
    .bind(&block.hash)
    .bind(network_id)
    .bind(to_i64(block.height, "block height")?)
    .bind(block.previousblockhash.as_deref())
    .bind(&block.merkleroot)
    .bind(block.blockcommitments.as_deref())
    .bind(block_time)
    .bind(to_i64(block.size, "block size")?)
    .bind(i32::try_from(block.tx.len()).map_err(|_| {
        ExplorerError::InvalidNodeResponse("too many transactions in block".to_owned())
    })?)
    .bind(&block.bits)
    .bind(difficulty_text)
    .bind(&block.nonce)
    .bind(
        block
            .chain_supply
            .as_ref()
            .and_then(|value| value.chain_value_zat),
    )
    .bind(indexed.fetched_at)
    .bind(&indexed.raw)
    .execute(&mut **transaction)
    .await?;

    let stored = sqlx::query(
        "SELECT network_id, height, previous_block_hash, merkle_root,
                block_commitments, block_time, size_bytes, transaction_count,
                bits, nonce, chain_supply_zat
         FROM blocks WHERE block_hash = $1",
    )
    .bind(&block.hash)
    .fetch_one(&mut **transaction)
    .await?;
    let immutable_block_matches = stored.try_get::<String, _>("network_id")? == network_id
        && stored.try_get::<i64, _>("height")? == to_i64(block.height, "block height")?
        && stored.try_get::<Option<String>, _>("previous_block_hash")? == block.previousblockhash
        && stored.try_get::<String, _>("merkle_root")? == block.merkleroot
        && stored.try_get::<Option<String>, _>("block_commitments")? == block.blockcommitments
        && stored.try_get::<DateTime<Utc>, _>("block_time")? == block_time
        && stored.try_get::<i64, _>("size_bytes")? == to_i64(block.size, "block size")?
        && stored.try_get::<i32, _>("transaction_count")?
            == i32::try_from(block.tx.len()).map_err(|_| {
                ExplorerError::InvalidNodeResponse("too many transactions in block".to_owned())
            })?
        && stored.try_get::<String, _>("bits")? == block.bits
        && stored.try_get::<String, _>("nonce")? == block.nonce
        && stored.try_get::<Option<i64>, _>("chain_supply_zat")?
            == block
                .chain_supply
                .as_ref()
                .and_then(|value| value.chain_value_zat);
    if !immutable_block_matches {
        return Err(ExplorerError::InvalidNodeResponse(format!(
            "immutable block facts changed for {}",
            block.hash
        )));
    }

    let (auxpow_bytes, exact_state, witness_confirmations, validation_state, verifier_version) =
        if let Some(aux) = &indexed.auxpow {
            (
                Some(decode_hex(&block.solution, 256 * 1024)?),
                aux.exact_witness_state.as_str(),
                aux.witness_confirmations.map(i64::from),
                aux.verification_state.as_str(),
                Some(aux.verifier_version.as_str()),
            )
        } else {
            (
                None,
                "native_genesis",
                Some(block.confirmations.max(0)),
                "genesis",
                None,
            )
        };
    sqlx::query(
        "INSERT INTO block_witnesses (
            block_hash, witness_hash, raw_block, auxpow_bytes, exact_witness_state,
            witness_confirmations, local_validation_state, verifier_version, indexed_at
         ) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9)
         ON CONFLICT (block_hash, witness_hash) DO UPDATE SET
            exact_witness_state = EXCLUDED.exact_witness_state,
            witness_confirmations = EXCLUDED.witness_confirmations,
            indexed_at = EXCLUDED.indexed_at",
    )
    .bind(&block.hash)
    .bind(&witness_hash)
    .bind(&indexed.raw_block)
    .bind(auxpow_bytes)
    .bind(exact_state)
    .bind(witness_confirmations)
    .bind(validation_state)
    .bind(verifier_version)
    .bind(indexed.fetched_at)
    .execute(&mut **transaction)
    .await?;

    for (position, rpc_transaction) in block.tx.iter().enumerate() {
        let transaction_id = insert_transaction(transaction, rpc_transaction).await?;
        sqlx::query(
            "INSERT INTO block_transactions (
                block_hash, witness_hash, tx_position, transaction_instance_id
             ) VALUES ($1,$2,$3,$4)
             ON CONFLICT (block_hash, witness_hash, tx_position) DO NOTHING",
        )
        .bind(&block.hash)
        .bind(&witness_hash)
        .bind(i32::try_from(position).map_err(|_| {
            ExplorerError::InvalidNodeResponse("transaction position overflow".to_owned())
        })?)
        .bind(transaction_id)
        .execute(&mut **transaction)
        .await?;
    }

    for pool in &block.value_pools {
        sqlx::query(
            "INSERT INTO value_pool_snapshots (
                block_hash, witness_hash, pool_id, chain_value_zat, value_delta_zat, monitored
             ) VALUES ($1,$2,$3,$4,$5,$6)
             ON CONFLICT (block_hash, witness_hash, pool_id) DO UPDATE SET
                chain_value_zat = EXCLUDED.chain_value_zat,
                value_delta_zat = EXCLUDED.value_delta_zat,
                monitored = EXCLUDED.monitored",
        )
        .bind(&block.hash)
        .bind(&witness_hash)
        .bind(&pool.id)
        .bind(pool.chain_value_zat)
        .bind(pool.value_delta_zat)
        .bind(pool.monitored)
        .execute(&mut **transaction)
        .await?;
    }

    if let Some(aux) = &indexed.auxpow {
        sqlx::query(
            "INSERT INTO auxpow_links (
                block_hash, witness_hash, proof_version, proof_size, parent_block_hash,
                parent_header_bits, parent_hash_meets_claimed_target, parent_coinbase_txid,
                parent_merkle_depth, parent_coinbase_index, auth_data_merkle_depth,
                auth_data_coinbase_index, auxiliary_merkle_depth, auxiliary_index,
                parent_lookup_state, parent_sources_agree, verified_at
             ) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16,$17)
             ON CONFLICT (block_hash, witness_hash) DO UPDATE SET
                parent_lookup_state = EXCLUDED.parent_lookup_state,
                parent_sources_agree = EXCLUDED.parent_sources_agree,
                verified_at = EXCLUDED.verified_at",
        )
        .bind(&block.hash)
        .bind(&witness_hash)
        .bind(i16::from(aux.proof_version))
        .bind(to_i64(aux.proof_size as u64, "proof size")?)
        .bind(&aux.parent_block_hash)
        .bind(&aux.parent_header_bits)
        .bind(aux.parent_hash_meets_claimed_target)
        .bind(&aux.parent_coinbase_txid)
        .bind(i32::try_from(aux.parent_merkle_depth).map_err(|_| {
            ExplorerError::InvalidNodeResponse("parent branch depth overflow".to_owned())
        })?)
        .bind(i64::from(aux.parent_coinbase_index))
        .bind(i32::try_from(aux.auth_data_merkle_depth).map_err(|_| {
            ExplorerError::InvalidNodeResponse("auth-data branch depth overflow".to_owned())
        })?)
        .bind(i64::from(aux.auth_data_coinbase_index))
        .bind(i32::try_from(aux.auxiliary_merkle_depth).map_err(|_| {
            ExplorerError::InvalidNodeResponse("auxiliary branch depth overflow".to_owned())
        })?)
        .bind(i64::from(aux.auxiliary_index))
        .bind(aux.parent_lookup_state.as_str())
        .bind(aux.parent_sources_agree)
        .bind(indexed.fetched_at)
        .execute(&mut **transaction)
        .await?;

        for observation in &aux.parent_observations {
            let parent_time = observation
                .block
                .as_ref()
                .and_then(|parent| DateTime::<Utc>::from_timestamp(parent.time, 0));
            sqlx::query(
                "INSERT INTO parent_chain_observations (
                    block_hash, witness_hash, source_name, observation_state,
                    parent_height, parent_confirmations, parent_time, parent_bits,
                    parent_difficulty_text, embedded_header_matches, checked_at
                 ) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11)
                 ON CONFLICT (block_hash, witness_hash, source_name) DO UPDATE SET
                    observation_state = EXCLUDED.observation_state,
                    parent_height = EXCLUDED.parent_height,
                    parent_confirmations = EXCLUDED.parent_confirmations,
                    parent_time = EXCLUDED.parent_time,
                    parent_bits = EXCLUDED.parent_bits,
                    parent_difficulty_text = EXCLUDED.parent_difficulty_text,
                    embedded_header_matches = EXCLUDED.embedded_header_matches,
                    checked_at = EXCLUDED.checked_at",
            )
            .bind(&block.hash)
            .bind(&witness_hash)
            .bind(&observation.source)
            .bind(observation.state.as_str())
            .bind(
                observation
                    .block
                    .as_ref()
                    .map(|parent| to_i64(parent.height, "parent height"))
                    .transpose()?,
            )
            .bind(
                observation
                    .block
                    .as_ref()
                    .map(|parent| parent.confirmations),
            )
            .bind(parent_time)
            .bind(
                observation
                    .block
                    .as_ref()
                    .map(|parent| parent.bits.as_str()),
            )
            .bind(
                observation
                    .block
                    .as_ref()
                    .map(|parent| scalar_text(&parent.difficulty)),
            )
            .bind(observation.embedded_header_matches)
            .bind(observation.checked_at)
            .execute(&mut **transaction)
            .await?;
        }
    }

    Ok(())
}

async fn insert_transaction(
    transaction: &mut Transaction<'_, Postgres>,
    rpc_transaction: &RpcTransaction,
) -> Result<i64> {
    let raw = rpc_transaction
        .hex
        .as_deref()
        .map(|value| decode_hex(value, 4 * 1024 * 1024))
        .transpose()?;
    let raw_hash = raw_transaction_hash(rpc_transaction, raw.as_deref());
    let auth_digest = rpc_transaction.authdigest.as_deref().unwrap_or(&raw_hash);
    ensure_hash("transaction ID", &rpc_transaction.txid)?;
    ensure_hash("transaction authorization digest", auth_digest)?;
    let is_coinbase = rpc_transaction
        .vin
        .iter()
        .any(|input| input.coinbase.is_some());
    let raw_rpc = serde_json::to_value(rpc_transaction).map_err(|error| {
        ExplorerError::InvalidNodeResponse(format!("could not preserve transaction JSON: {error}"))
    })?;
    let id: i64 = sqlx::query_scalar(
        "INSERT INTO transaction_instances (
            txid, auth_digest, raw_hash, raw_transaction, version, size_bytes,
            lock_time, expiry_height, is_coinbase, value_balance_zat,
            sapling_spend_count, sapling_output_count, orchard_action_count,
            ironwood_action_count, raw_rpc
         ) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15)
         ON CONFLICT (txid, auth_digest) DO UPDATE SET txid = EXCLUDED.txid
         RETURNING transaction_instance_id",
    )
    .bind(&rpc_transaction.txid)
    .bind(auth_digest)
    .bind(&raw_hash)
    .bind(raw)
    .bind(rpc_transaction.version)
    .bind(to_i64(rpc_transaction.size, "transaction size")?)
    .bind(to_i64(rpc_transaction.locktime, "lock time")?)
    .bind(to_i64(rpc_transaction.expiryheight, "expiry height")?)
    .bind(is_coinbase)
    .bind(rpc_transaction.value_balance_zat)
    .bind(to_i32(
        rpc_transaction.shielded_spends.len(),
        "Sapling spend count",
    )?)
    .bind(to_i32(
        rpc_transaction.shielded_outputs.len(),
        "Sapling output count",
    )?)
    .bind(bundle_action_count(rpc_transaction.orchard.as_ref()))
    .bind(bundle_action_count(rpc_transaction.ironwood.as_ref()))
    .bind(raw_rpc)
    .fetch_one(&mut **transaction)
    .await?;

    let stored = sqlx::query(
        "SELECT raw_hash, version, size_bytes, lock_time, expiry_height,
                is_coinbase, value_balance_zat
         FROM transaction_instances WHERE transaction_instance_id = $1",
    )
    .bind(id)
    .fetch_one(&mut **transaction)
    .await?;
    if stored.try_get::<String, _>("raw_hash")? != raw_hash
        || stored.try_get::<i64, _>("version")? != rpc_transaction.version
        || stored.try_get::<i64, _>("size_bytes")?
            != to_i64(rpc_transaction.size, "transaction size")?
        || stored.try_get::<i64, _>("lock_time")? != to_i64(rpc_transaction.locktime, "lock time")?
        || stored.try_get::<i64, _>("expiry_height")?
            != to_i64(rpc_transaction.expiryheight, "expiry height")?
        || stored.try_get::<bool, _>("is_coinbase")? != is_coinbase
        || stored.try_get::<Option<i64>, _>("value_balance_zat")?
            != rpc_transaction.value_balance_zat
    {
        return Err(ExplorerError::InvalidNodeResponse(format!(
            "immutable transaction facts changed for {}:{}",
            rpc_transaction.txid, auth_digest
        )));
    }

    for (position, input) in rpc_transaction.vin.iter().enumerate() {
        sqlx::query(
            "INSERT INTO transparent_inputs (
                transaction_instance_id, input_index, previous_txid,
                previous_output_index, coinbase_data, sequence
             ) VALUES ($1,$2,$3,$4,$5,$6)
             ON CONFLICT (transaction_instance_id, input_index) DO NOTHING",
        )
        .bind(id)
        .bind(to_i32(position, "input index")?)
        .bind(input.txid.as_deref())
        .bind(
            input
                .vout
                .map(|value| {
                    i32::try_from(value).map_err(|_| {
                        ExplorerError::InvalidNodeResponse(
                            "previous output index exceeds the database range".to_owned(),
                        )
                    })
                })
                .transpose()?,
        )
        .bind(input.coinbase.as_deref())
        .bind(
            input
                .sequence
                .map(|value| to_i64(value, "input sequence"))
                .transpose()?,
        )
        .execute(&mut **transaction)
        .await?;
    }
    for output in &rpc_transaction.vout {
        sqlx::query(
            "INSERT INTO transparent_outputs (
                transaction_instance_id, output_index, value_zat, address,
                script_type, script_hex
             ) VALUES ($1,$2,$3,$4,$5,$6)
             ON CONFLICT (transaction_instance_id, output_index) DO NOTHING",
        )
        .bind(id)
        .bind(
            i32::try_from(output.n).map_err(|_| {
                ExplorerError::InvalidNodeResponse("output index overflow".to_owned())
            })?,
        )
        .bind(output.value_zat)
        .bind(output.script_pub_key.primary_address())
        .bind(output.script_pub_key.script_type.as_deref())
        .bind(output.script_pub_key.hex.as_deref())
        .execute(&mut **transaction)
        .await?;
    }
    Ok(id)
}

async fn select_canonical(
    transaction: &mut Transaction<'_, Postgres>,
    network_id: &str,
    indexed: &IndexedBlock,
) -> Result<()> {
    let witness_hash = witness_hash(indexed);
    insert_canonical_selector(transaction, network_id, indexed).await?;

    let row = sqlx::query(
        "SELECT block_hash, witness_hash FROM canonical_chain
         WHERE network_id = $1 AND height = $2",
    )
    .bind(network_id)
    .bind(to_i64(indexed.block.height, "block height")?)
    .fetch_one(&mut **transaction)
    .await?;
    let selected_hash: String = row.try_get("block_hash")?;
    let selected_witness: String = row.try_get("witness_hash")?;
    if selected_hash != indexed.block.hash || selected_witness != witness_hash {
        return Err(ExplorerError::NotReady(
            "canonical height changed before the block could be committed".to_owned(),
        ));
    }
    sqlx::query(
        "UPDATE chain_state SET
            indexed_height = $2,
            indexed_hash = $3,
            indexed_witness_hash = $4,
            status = 'syncing',
            last_error = NULL,
            updated_at = now()
         WHERE network_id = $1",
    )
    .bind(network_id)
    .bind(to_i64(indexed.block.height, "block height")?)
    .bind(&indexed.block.hash)
    .bind(witness_hash)
    .execute(&mut **transaction)
    .await?;
    Ok(())
}

async fn insert_canonical_selector(
    transaction: &mut Transaction<'_, Postgres>,
    network_id: &str,
    indexed: &IndexedBlock,
) -> Result<()> {
    sqlx::query(
        "INSERT INTO canonical_chain (network_id, height, block_hash, witness_hash)
         VALUES ($1,$2,$3,$4)
         ON CONFLICT (network_id, height) DO NOTHING",
    )
    .bind(network_id)
    .bind(to_i64(indexed.block.height, "block height")?)
    .bind(&indexed.block.hash)
    .bind(witness_hash(indexed))
    .execute(&mut **transaction)
    .await?;
    Ok(())
}

fn validate_replacement_branch(
    ancestor: Option<&(u64, String)>,
    replacement: &[IndexedBlock],
) -> Result<()> {
    let mut expected_height = ancestor.map_or(0, |(height, _)| height.saturating_add(1));
    let mut expected_previous = ancestor.map(|(_, hash)| hash.as_str());
    for indexed in replacement {
        if indexed.block.height != expected_height
            || indexed.block.previousblockhash.as_deref() != expected_previous
        {
            return Err(ExplorerError::NotReady(
                "staged reorganization branch is not contiguous".to_owned(),
            ));
        }
        expected_height = expected_height.saturating_add(1);
        expected_previous = Some(indexed.block.hash.as_str());
    }
    Ok(())
}

fn witness_hash(indexed: &IndexedBlock) -> String {
    indexed.auxpow.as_ref().map_or_else(
        || crate::auxpow::block_witness_hash(&indexed.raw_block),
        |aux| aux.witness_hash.clone(),
    )
}

fn raw_transaction_hash(transaction: &RpcTransaction, raw: Option<&[u8]>) -> String {
    let mut hasher = Sha256::new();
    hasher.update(RAW_TX_HASH_DOMAIN);
    if let Some(raw) = raw {
        hasher.update(raw);
    } else {
        hasher.update(transaction.txid.as_bytes());
        if let Some(auth_digest) = &transaction.authdigest {
            hasher.update(auth_digest.as_bytes());
        }
    }
    hex::encode(hasher.finalize())
}

fn bundle_action_count(value: Option<&Value>) -> i32 {
    let Some(value) = value else { return 0 };
    match value {
        Value::Array(actions) => i32::try_from(actions.len()).unwrap_or(i32::MAX),
        Value::Object(object) => object
            .get("actions")
            .and_then(|actions| match actions {
                Value::Array(actions) => i32::try_from(actions.len()).ok(),
                Value::Number(number) => {
                    number.as_u64().and_then(|count| i32::try_from(count).ok())
                }
                _ => None,
            })
            .unwrap_or(0),
        _ => 0,
    }
}

fn scalar_text(value: &Value) -> String {
    match value {
        Value::String(value) => value.clone(),
        Value::Number(value) => value.to_string(),
        other => other.to_string(),
    }
}

fn ensure_hash(label: &str, value: &str) -> Result<()> {
    if value.len() != 64 || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(ExplorerError::InvalidNodeResponse(format!(
            "{label} is not a 32-byte hexadecimal identifier"
        )));
    }
    Ok(())
}

fn decode_hex(value: &str, maximum_bytes: usize) -> Result<Vec<u8>> {
    if !value.len().is_multiple_of(2) || value.len() / 2 > maximum_bytes {
        return Err(ExplorerError::InvalidNodeResponse(
            "hexadecimal payload exceeded its size bound".to_owned(),
        ));
    }
    hex::decode(value).map_err(|error| {
        ExplorerError::InvalidNodeResponse(format!("invalid hexadecimal payload: {error}"))
    })
}

fn to_i64(value: u64, label: &str) -> Result<i64> {
    i64::try_from(value).map_err(|_| {
        ExplorerError::InvalidNodeResponse(format!("{label} exceeds the database range"))
    })
}

fn to_i32(value: usize, label: &str) -> Result<i32> {
    i32::try_from(value).map_err(|_| {
        ExplorerError::InvalidNodeResponse(format!("{label} exceeds the database range"))
    })
}

fn truncate(value: &str, maximum_chars: usize) -> String {
    value.chars().take(maximum_chars).collect()
}
