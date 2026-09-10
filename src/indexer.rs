//! Reorg-aware canonical-chain indexer.

use std::time::Duration;

use chrono::Utc;
use tokio::sync::oneshot;
use tokio::time::MissedTickBehavior;
use tracing::{debug, error, info, warn};

use crate::{
    auxpow::AuxPowVerifier,
    config::NetworkConfig,
    db::{ChainTip, Database},
    error::{ExplorerError, Result},
    models::IndexedBlock,
    rpc::ZebraRpc,
};

const ZERO_HASH: &str = "0000000000000000000000000000000000000000000000000000000000000000";

/// Single-writer Wcash chain synchronizer.
#[derive(Clone)]
pub struct Indexer {
    database: Database,
    wcash: ZebraRpc,
    auxpow: AuxPowVerifier,
    network: NetworkConfig,
    poll_interval: Duration,
    max_reorg_depth: u64,
    evidence_refresh_depth: u64,
}

impl Indexer {
    pub fn new(
        database: Database,
        wcash: ZebraRpc,
        auxpow: AuxPowVerifier,
        network: NetworkConfig,
        poll_interval: Duration,
        max_reorg_depth: u64,
        evidence_refresh_depth: u64,
    ) -> Self {
        Self {
            database,
            wcash,
            auxpow,
            network,
            poll_interval,
            max_reorg_depth,
            evidence_refresh_depth,
        }
    }

    /// Runs until shutdown, retaining an advisory-lock connection for exclusivity.
    pub async fn run(self) -> Result<()> {
        let mut writer_lease = self.database.acquire_writer_lock().await?;
        let (lock_lost_tx, mut lock_lost_rx) = oneshot::channel();
        let lock_monitor = tokio::spawn(async move {
            loop {
                tokio::time::sleep(Duration::from_secs(1)).await;
                if !matches!(
                    Database::writer_lock_is_held(&mut writer_lease).await,
                    Ok(true)
                ) {
                    let _ = lock_lost_tx.send(());
                    break;
                }
            }
        });
        info!(network = %self.network.id, "explorer indexer acquired writer lease");
        let mut interval = tokio::time::interval(self.poll_interval);
        interval.set_missed_tick_behavior(MissedTickBehavior::Skip);
        loop {
            tokio::select! {
                _ = &mut lock_lost_rx => {
                    lock_monitor.abort();
                    return Err(ExplorerError::NotReady(
                        "PostgreSQL writer lease was lost; process restart is required".to_owned(),
                    ));
                }
                _ = interval.tick() => {
                    let sync = self.sync_once();
                    tokio::pin!(sync);
                    tokio::select! {
                        _ = &mut lock_lost_rx => {
                            lock_monitor.abort();
                            return Err(ExplorerError::NotReady(
                                "PostgreSQL writer lease was lost during synchronization; process restart is required".to_owned(),
                            ));
                        }
                        outcome = &mut sync => {
                            if let Err(error) = outcome {
                                error!(error = %error, "chain synchronization failed");
                                if let Err(state_error) = self
                                    .database
                                    .mark_indexer_error(&self.network.id, &error.to_string())
                                    .await
                                {
                                    error!(error = %state_error, "could not persist indexer failure state");
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    /// Synchronizes the database to one stable snapshot of the Wcash tip.
    pub async fn sync_once(&self) -> Result<SyncOutcome> {
        let info = self.wcash.blockchain_info().await?;
        let node_tip_hash = self.wcash.block_hash(info.blocks).await?;
        self.validate_identity(&info.chain).await?;
        let mut tip = self.database.canonical_tip(&self.network.id).await?;
        self.database
            .observe_node_tip(&self.network.id, info.blocks, &node_tip_hash)
            .await?;

        let mut reorg_depth = 0;
        let mut indexed_blocks = 0_u64;
        let mut replaced_branch = false;
        if let Some(stored_tip) = &tip {
            let canonical_matches = if stored_tip.height <= info.blocks {
                self.wcash.block_hash(stored_tip.height).await? == stored_tip.block_hash
            } else {
                false
            };
            if !canonical_matches {
                let ancestor = self.find_common_ancestor(stored_tip, info.blocks).await?;
                let ancestor_height = ancestor.as_ref().map_or(0, |(height, _)| *height);
                reorg_depth = stored_tip
                    .height
                    .saturating_sub(ancestor.as_ref().map_or(0, |(height, _)| *height));
                warn!(
                    old_height = stored_tip.height,
                    ancestor_height, reorg_depth, "canonical chain divergence detected"
                );
                let start = ancestor
                    .as_ref()
                    .map_or(0, |(height, _)| height.saturating_add(1));
                let mut expected_previous = ancestor.as_ref().map(|(_, hash)| hash.clone());
                let mut replacement = Vec::new();
                for height in start..=info.blocks {
                    let indexed = self
                        .fetch_indexed_block(height, expected_previous.as_deref())
                        .await?;
                    expected_previous = Some(indexed.block.hash.clone());
                    replacement.push(indexed);
                }
                if self.wcash.block_hash(info.blocks).await? != node_tip_hash {
                    return Err(ExplorerError::NotReady(
                        "Wcash tip changed while staging a replacement branch".to_owned(),
                    ));
                }
                if let Some((height, hash)) = &ancestor
                    && self.wcash.block_hash(*height).await? != *hash
                {
                    return Err(ExplorerError::NotReady(
                        "Wcash common ancestor changed while staging a replacement branch"
                            .to_owned(),
                    ));
                }
                self.database
                    .replace_canonical_branch(&self.network.id, stored_tip, ancestor, &replacement)
                    .await?;
                indexed_blocks = u64::try_from(replacement.len()).unwrap_or(u64::MAX);
                replaced_branch = true;
                tip = self.database.canonical_tip(&self.network.id).await?;
            }
        }

        if !replaced_branch {
            let start = tip.as_ref().map_or(0, |tip| tip.height.saturating_add(1));
            let mut expected_previous = tip.as_ref().map(|tip| tip.block_hash.clone());
            for height in start..=info.blocks {
                let indexed = self
                    .fetch_indexed_block(height, expected_previous.as_deref())
                    .await?;
                expected_previous = Some(indexed.block.hash.clone());
                self.database
                    .commit_block(&self.network.id, &indexed)
                    .await?;
                indexed_blocks = indexed_blocks.saturating_add(1);
            }
        }

        if let Some((height, _)) = self
            .database
            .evidence_refresh_candidate(&self.network.id, self.evidence_refresh_depth)
            .await?
        {
            let expected_previous = if height == 0 {
                None
            } else {
                Some(self.wcash.block_hash(height - 1).await?)
            };
            let refreshed = self
                .fetch_indexed_block(height, expected_previous.as_deref())
                .await?;
            self.database
                .refresh_block_evidence(&self.network.id, &refreshed)
                .await?;
        }

        let final_tip = self.database.canonical_tip(&self.network.id).await?;
        let final_height = final_tip.as_ref().map_or(0, |tip| tip.height);
        let final_hash = final_tip
            .as_ref()
            .map_or(self.network.genesis_hash.as_str(), |tip| {
                tip.block_hash.as_str()
            });
        self.database
            .update_node_tip(
                &self.network.id,
                info.blocks,
                &node_tip_hash,
                if final_height == info.blocks {
                    "ready"
                } else {
                    "syncing"
                },
                None,
            )
            .await?;
        if indexed_blocks > 0 || reorg_depth > 0 {
            info!(
                indexed_blocks,
                indexed_height = final_height,
                indexed_hash = %final_hash,
                node_height = info.blocks,
                reorg_depth,
                "chain synchronization completed"
            );
        } else {
            debug!(
                indexed_height = final_height,
                indexed_hash = %final_hash,
                node_height = info.blocks,
                "chain synchronization is current"
            );
        }
        Ok(SyncOutcome {
            indexed_blocks,
            indexed_height: final_height,
            node_height: info.blocks,
            reorg_depth,
        })
    }

    async fn validate_identity(&self, node_chain: &str) -> Result<()> {
        let node_genesis = self.wcash.block_hash(0).await?;
        if node_genesis != self.network.genesis_hash {
            return Err(ExplorerError::Config(format!(
                "Wcash RPC genesis mismatch: expected {}, received {}",
                self.network.genesis_hash, node_genesis
            )));
        }
        let expected_chain = match self.network.id.as_str() {
            "mainnet" => "main",
            "testnet" => "test",
            configured => configured,
        };
        if node_chain != expected_chain {
            return Err(ExplorerError::Config(format!(
                "Wcash RPC network mismatch: expected {expected_chain}, received {node_chain}"
            )));
        }
        Ok(())
    }

    async fn fetch_indexed_block(
        &self,
        height: u64,
        expected_previous: Option<&str>,
    ) -> Result<IndexedBlock> {
        let expected_hash = self.wcash.block_hash(height).await?;
        let (raw_block, (block, raw)) = tokio::try_join!(
            self.wcash.block_raw(&expected_hash),
            self.wcash.block_verbose(&expected_hash)
        )?;
        if block.hash != expected_hash || block.height != height {
            return Err(ExplorerError::InvalidNodeResponse(format!(
                "node returned inconsistent block identity at height {height}"
            )));
        }
        let previous_matches = if height == 0 {
            block.previousblockhash.is_none()
                || block
                    .previousblockhash
                    .as_deref()
                    .is_some_and(|hash| hash == ZERO_HASH)
        } else {
            block.previousblockhash.as_deref() == expected_previous
        };
        if !previous_matches {
            return Err(ExplorerError::NotReady(format!(
                "chain changed while fetching height {height}; retrying"
            )));
        }
        let auxpow = self.auxpow.verify(&block, &raw_block).await?;
        if self.wcash.block_hash(height).await? != expected_hash {
            return Err(ExplorerError::NotReady(format!(
                "chain changed while validating height {height}; retrying"
            )));
        }
        Ok(IndexedBlock {
            block,
            auxpow,
            fetched_at: Utc::now(),
            raw_block,
            raw,
        })
    }

    async fn find_common_ancestor(
        &self,
        stored_tip: &ChainTip,
        node_height: u64,
    ) -> Result<Option<(u64, String)>> {
        let mut height = stored_tip.height.min(node_height);
        let mut examined = 0_u64;
        loop {
            if examined > self.max_reorg_depth {
                return Err(ExplorerError::NotReady(format!(
                    "chain divergence exceeds MAX_REORG_DEPTH ({})",
                    self.max_reorg_depth
                )));
            }
            let stored = self
                .database
                .canonical_hash(&self.network.id, height)
                .await?;
            let node = self.wcash.block_hash(height).await?;
            if stored.as_deref() == Some(node.as_str()) {
                return Ok(Some((height, node)));
            }
            if height == 0 {
                return Err(ExplorerError::Config(
                    "stored and node genesis blocks differ".to_owned(),
                ));
            }
            height -= 1;
            examined += 1;
        }
    }
}

/// Observable result of one deterministic synchronization pass.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SyncOutcome {
    pub indexed_blocks: u64,
    pub indexed_height: u64,
    pub node_height: u64,
    pub reorg_depth: u64,
}
