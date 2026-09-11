//! Canonical-chain analytics used by charts and transparent-address views.
//!
//! These queries deliberately derive state from `canonical_chain` instead of
//! persisting reorg-sensitive rollups. Detached blocks remain available for
//! audit without leaking into public totals.

use std::{sync::Arc, time::Duration};

use axum::{
    BoxError, Json, Router,
    error_handling::HandleErrorLayer,
    extract::{Query, State},
    http::{HeaderValue, header},
    response::{IntoResponse, Response},
    routing::get,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use tower::{ServiceBuilder, limit::ConcurrencyLimitLayer, timeout::TimeoutLayer};
use tower_http::set_header::SetResponseHeaderLayer;

use crate::{
    api::{
        AmountView, AppState, address, amount, amount_from_zatoshi_text, envelope, read_snapshot,
        snapshot_chain_state,
    },
    error::{ExplorerError, Result},
    models::ApiEnvelope,
};

const DEFAULT_HISTORY_LIMIT: u16 = 240;
const MAX_HISTORY_LIMIT: u16 = 2_048;
const DEFAULT_RICH_LIST_LIMIT: u16 = 50;
const MAX_RICH_LIST_LIMIT: u16 = 100;
const ANALYTICS_CONCURRENCY_LIMIT: usize = 4;

pub(crate) fn routes() -> Router<Arc<AppState>> {
    Router::new()
        .route("/api/v1/network/history", get(network_history))
        .route("/api/v1/value-pools/history", get(value_pool_history))
        .route("/api/v1/addresses/{address}", get(address))
        .route("/api/v1/addresses/stats", get(address_stats))
        .route("/api/v1/addresses/rich-list", get(rich_list))
        .route("/api/v1/merge-mining/stats", get(merge_mining_stats))
        .layer(
            ServiceBuilder::new()
                .layer(HandleErrorLayer::new(analytics_capacity_error))
                .layer(TimeoutLayer::new(Duration::from_secs(8)))
                .layer(ConcurrencyLimitLayer::new(ANALYTICS_CONCURRENCY_LIMIT))
                .layer(SetResponseHeaderLayer::if_not_present(
                    header::CACHE_CONTROL,
                    HeaderValue::from_static("public, max-age=5, stale-while-revalidate=15"),
                )),
        )
}

async fn analytics_capacity_error(_error: BoxError) -> Response {
    let mut response = ExplorerError::NotReady(
        "analytics capacity is temporarily busy; retry the request".to_owned(),
    )
    .into_response();
    response
        .headers_mut()
        .insert(header::RETRY_AFTER, HeaderValue::from_static("2"));
    response
}

#[derive(Debug, Deserialize)]
struct HistoryQuery {
    before: Option<u64>,
    limit: Option<u16>,
}

#[derive(Debug, Deserialize)]
struct RichListQuery {
    limit: Option<u16>,
}

async fn network_history(
    State(state): State<Arc<AppState>>,
    Query(query): Query<HistoryQuery>,
) -> Result<Json<ApiEnvelope<NetworkHistoryView>>> {
    let limit = history_limit(query.limit);
    let before = query
        .before
        .map(|value| i64::try_from(value).map_err(|_| height_error()))
        .transpose()?;
    let mut transaction = read_snapshot(&state).await?;
    let chain = snapshot_chain_state(&mut transaction, &state.network.id).await?;
    let rows = sqlx::query_as::<_, NetworkHistoryRow>(NETWORK_HISTORY_QUERY)
        .bind(&state.network.id)
        .bind(before)
        .bind(i64::from(limit))
        .fetch_all(&mut *transaction)
        .await?;
    transaction.commit().await?;
    let points = rows
        .into_iter()
        .map(|row| {
            Ok(NetworkHistoryPoint {
                height: as_height(row.height)?,
                hash: row.hash,
                time: row.block_time,
                difficulty: row.difficulty_text,
                spacing_seconds: row.spacing_seconds,
                size_bytes: row.size_bytes,
                transaction_count: row.transaction_count,
                total_issued: row
                    .chain_supply_zat
                    .map(|value| amount(value, &state.network)),
            })
        })
        .collect::<Result<Vec<_>>>()?;
    let data = NetworkHistoryView {
        as_of_height: optional_height(chain.indexed_height)?,
        as_of_hash: chain.indexed_hash.clone(),
        target_spacing_seconds: state.network.target_spacing_seconds,
        points,
    };
    Ok(Json(envelope(&state, data, None, &chain)))
}

async fn value_pool_history(
    State(state): State<Arc<AppState>>,
    Query(query): Query<HistoryQuery>,
) -> Result<Json<ApiEnvelope<ValuePoolHistoryView>>> {
    let limit = history_limit(query.limit);
    let before = query
        .before
        .map(|value| i64::try_from(value).map_err(|_| height_error()))
        .transpose()?;
    let mut transaction = read_snapshot(&state).await?;
    let chain = snapshot_chain_state(&mut transaction, &state.network.id).await?;
    let rows = sqlx::query_as::<_, ValuePoolHistoryRow>(VALUE_POOL_HISTORY_QUERY)
        .bind(&state.network.id)
        .bind(before)
        .bind(i64::from(limit))
        .fetch_all(&mut *transaction)
        .await?;
    let points = rows
        .into_iter()
        .map(|row| {
            Ok(ValuePoolHistoryPoint {
                height: as_height(row.height)?,
                hash: row.hash,
                time: row.block_time,
                total_issued: row
                    .chain_supply_zat
                    .map(|value| amount(value, &state.network)),
                transparent: pool_snapshot(
                    "transparent",
                    row.transparent_chain_value_zat,
                    row.transparent_value_delta_zat,
                    row.transparent_monitored,
                    &state,
                ),
                ironwood: pool_snapshot(
                    "ironwood",
                    row.ironwood_chain_value_zat,
                    row.ironwood_value_delta_zat,
                    row.ironwood_monitored,
                    &state,
                ),
            })
        })
        .collect::<Result<Vec<_>>>()?;
    let unexpected_pool_samples: i64 = sqlx::query_scalar(
        "SELECT COUNT(*)::BIGINT
         FROM canonical_chain c
         JOIN value_pool_snapshots p
           ON p.block_hash = c.block_hash AND p.witness_hash = c.witness_hash
         WHERE c.network_id = $1
           AND p.pool_id NOT IN ('transparent', 'ironwood')
           AND (
               p.monitored IS TRUE
               OR COALESCE(p.chain_value_zat, 0) <> 0
               OR COALESCE(p.value_delta_zat, 0) <> 0
           )",
    )
    .bind(&state.network.id)
    .fetch_one(&mut *transaction)
    .await?;
    transaction.commit().await?;
    let data = ValuePoolHistoryView {
        as_of_height: optional_height(chain.indexed_height)?,
        as_of_hash: chain.indexed_hash.clone(),
        unexpected_pool_samples,
        scope_notice: "Wcash publishes the transparent pool and the current Ironwood shielded pool. Individual shielded addresses and balances are not public chain data.".to_owned(),
        points,
    };
    Ok(Json(envelope(&state, data, None, &chain)))
}

async fn merge_mining_stats(
    State(state): State<Arc<AppState>>,
) -> Result<Json<ApiEnvelope<MergeMiningStatsView>>> {
    let mut transaction = read_snapshot(&state).await?;
    let chain = snapshot_chain_state(&mut transaction, &state.network.id).await?;
    let row = sqlx::query_as::<_, MergeMiningStatsRow>(MERGE_MINING_STATS_QUERY)
        .bind(&state.network.id)
        .fetch_one(&mut *transaction)
        .await?;
    transaction.commit().await?;
    let data = MergeMiningStatsView {
        as_of_height: optional_height(chain.indexed_height)?,
        as_of_hash: chain.indexed_hash.clone(),
        eligible_child_blocks: row.eligible_child_blocks,
        auxpow_blocks: row.auxpow_blocks,
        locally_verified_blocks: row.locally_verified_blocks,
        parent_target_verified_blocks: row.parent_target_verified_blocks,
        canonical_parent_blocks: row.canonical_parent_blocks,
        orphaned_parent_blocks: row.orphaned_parent_blocks,
        not_found_parent_blocks: row.not_found_parent_blocks,
        unavailable_parent_blocks: row.unavailable_parent_blocks,
        disagreement_parent_blocks: row.disagreement_parent_blocks,
        parent_quorum_agreement_blocks: row.parent_quorum_agreement_blocks,
        best_chain_witness_blocks: row.best_chain_witness_blocks,
        fully_verified_blocks: row.fully_verified_blocks,
        locally_verified_without_parent_observation_blocks: row
            .locally_verified_without_parent_observation_blocks,
        anomaly_blocks: row.anomaly_blocks,
        observation_source_count: row.observation_source_count,
        last_verified_at: row.last_verified_at,
        scope_notice: "Counts cover every canonical non-genesis Wcash block. A locally verified Wcash AuxPoW witness is not anomalous when Zcash parent observation is not configured; canonical-parent evidence remains a separate, stronger status.".to_owned(),
    };
    Ok(Json(envelope(&state, data, None, &chain)))
}

async fn rich_list(
    State(state): State<Arc<AppState>>,
    Query(query): Query<RichListQuery>,
) -> Result<Json<ApiEnvelope<RichListView>>> {
    let limit = query
        .limit
        .unwrap_or(DEFAULT_RICH_LIST_LIMIT)
        .clamp(1, MAX_RICH_LIST_LIMIT);
    let mut transaction = read_snapshot(&state).await?;
    let chain = snapshot_chain_state(&mut transaction, &state.network.id).await?;
    let rows = sqlx::query_as::<_, RichListRow>(RICH_LIST_QUERY)
        .bind(&state.network.id)
        .bind(i64::from(limit))
        .fetch_all(&mut *transaction)
        .await?;
    let pool = sqlx::query_as::<_, LatestPoolRow>(LATEST_TRANSPARENT_POOL_QUERY)
        .bind(&state.network.id)
        .fetch_optional(&mut *transaction)
        .await?;
    transaction.commit().await?;

    let transparent_pool_zat = pool.as_ref().and_then(|row| row.chain_value_zat);
    let transparent_pool_monitored = pool.as_ref().and_then(|row| row.monitored);
    let transparent_pool_reported = transparent_pool_zat.is_some();
    let totals = rows
        .first()
        .map_or_else(RichListTotals::default, |row| RichListTotals {
            funded_address_count: row.funded_address_count,
            addressed_balance_zat: row.addressed_balance_zat,
            top_1_zat: row.top_1_zat,
            top_10_zat: row.top_10_zat,
            top_100_zat: row.top_100_zat,
        });
    let addresses = rows
        .into_iter()
        .map(|row| {
            Ok(RichListEntry {
                rank: u64::try_from(row.rank).unwrap_or(0),
                address: row.address,
                balance: amount(row.balance_zat, &state.network),
                total_received: amount_from_zatoshi_text(&row.total_received_zat, &state.network)?,
                total_sent: amount_from_zatoshi_text(&row.total_sent_zat, &state.network)?,
                transparent_pool_share_percent: transparent_pool_zat
                    .filter(|denominator| *denominator > 0)
                    .map(|denominator| exact_percent(row.balance_zat, denominator)),
                utxo_count: row.utxo_count,
                transaction_count: row.transaction_count,
                first_seen_height: row
                    .first_seen_height
                    .and_then(|value| value.try_into().ok()),
                last_seen_height: row.last_seen_height.and_then(|value| value.try_into().ok()),
                last_seen_at: row.last_seen_at,
            })
        })
        .collect::<Result<Vec<_>>>()?;
    let unassigned =
        unassigned_transparent_balance(transparent_pool_zat, totals.addressed_balance_zat)?;
    let data = RichListView {
        as_of_height: optional_height(chain.indexed_height)?,
        as_of_hash: chain.indexed_hash.clone(),
        transparent_pool: transparent_pool_zat.map(|value| amount(value, &state.network)),
        transparent_pool_reported,
        transparent_pool_monitored,
        funded_address_count: totals.funded_address_count,
        addressed_balance: amount(totals.addressed_balance_zat, &state.network),
        addressless_or_undecoded_balance: unassigned.map(|value| amount(value, &state.network)),
        top_1_balance: amount(totals.top_1_zat, &state.network),
        top_10_balance: amount(totals.top_10_zat, &state.network),
        top_100_balance: amount(totals.top_100_zat, &state.network),
        addresses,
        scope_notice: "Transparent balances only. This is not a ranking of shielded Wcash holders; shielded identities and balances cannot be derived from public chain data.".to_owned(),
    };
    Ok(Json(envelope(&state, data, None, &chain)))
}

async fn address_stats(
    State(state): State<Arc<AppState>>,
    Query(query): Query<HistoryQuery>,
) -> Result<Json<ApiEnvelope<AddressStatsView>>> {
    let limit = history_limit(query.limit);
    let before = query
        .before
        .map(|value| i64::try_from(value).map_err(|_| height_error()))
        .transpose()?;
    let mut transaction = read_snapshot(&state).await?;
    let chain = snapshot_chain_state(&mut transaction, &state.network.id).await?;
    let rows = sqlx::query_as::<_, AddressStatsRow>(ADDRESS_STATS_QUERY)
        .bind(&state.network.id)
        .bind(before)
        .bind(i64::from(limit))
        .fetch_all(&mut *transaction)
        .await?;
    let current = sqlx::query_as::<_, CurrentAddressStatsRow>(CURRENT_ADDRESS_STATS_QUERY)
        .bind(&state.network.id)
        .fetch_one(&mut *transaction)
        .await?;
    let pool = sqlx::query_as::<_, LatestPoolRow>(LATEST_TRANSPARENT_POOL_QUERY)
        .bind(&state.network.id)
        .fetch_optional(&mut *transaction)
        .await?;
    transaction.commit().await?;
    let transparent_pool_zat = pool.as_ref().and_then(|row| row.chain_value_zat);
    let transparent_pool_monitored = pool.as_ref().and_then(|row| row.monitored);
    let transparent_pool_reported = transparent_pool_zat.is_some();
    let data = AddressStatsView {
        as_of_height: optional_height(chain.indexed_height)?,
        as_of_hash: chain.indexed_hash.clone(),
        funded_address_count: current.funded_address_count,
        seen_address_count: current.seen_address_count,
        addressed_balance: amount(current.addressed_balance_zat, &state.network),
        transparent_pool: transparent_pool_zat.map(|value| amount(value, &state.network)),
        transparent_pool_reported,
        transparent_pool_monitored,
        addressless_or_undecoded_balance: unassigned_transparent_balance(
            transparent_pool_zat,
            current.addressed_balance_zat,
        )?
        .map(|value| amount(value, &state.network)),
        points: rows
            .into_iter()
            .map(|row| {
                Ok(AddressStatsPoint {
                    height: as_height(row.height)?,
                    time: row.block_time,
                    active_addresses: row.active_addresses,
                    new_addresses: row.new_addresses,
                    total_seen_addresses: row.total_seen_addresses,
                })
            })
            .collect::<Result<Vec<_>>>()?,
        scope_notice: "Counts cover decoded transparent Wcash addresses only. The protocol does not reveal a count of shielded holders.".to_owned(),
    };
    Ok(Json(envelope(&state, data, None, &chain)))
}

fn pool_snapshot(
    id: &str,
    chain_value_zat: Option<i64>,
    value_delta_zat: Option<i64>,
    monitored: Option<bool>,
    state: &AppState,
) -> PoolSnapshotView {
    PoolSnapshotView {
        id: id.to_owned(),
        chain_value: chain_value_zat.map(|value| amount(value, &state.network)),
        value_delta: value_delta_zat.map(|value| amount(value, &state.network)),
        reported: chain_value_zat.is_some(),
        monitored,
    }
}

fn history_limit(limit: Option<u16>) -> u16 {
    limit
        .unwrap_or(DEFAULT_HISTORY_LIMIT)
        .clamp(2, MAX_HISTORY_LIMIT)
}

fn as_height(value: i64) -> Result<u64> {
    u64::try_from(value).map_err(|_| {
        ExplorerError::InvalidNodeResponse("negative canonical height in explorer index".to_owned())
    })
}

fn optional_height(value: Option<i64>) -> Result<Option<u64>> {
    value.map(as_height).transpose()
}

fn height_error() -> ExplorerError {
    ExplorerError::InvalidRequest("height exceeds the supported range".to_owned())
}

fn exact_percent(numerator: i64, denominator: i64) -> String {
    if denominator <= 0 {
        return "0.00000000".to_owned();
    }
    let scaled = i128::from(numerator)
        .saturating_mul(100)
        .saturating_mul(100_000_000)
        / i128::from(denominator);
    format!("{}.{:08}", scaled / 100_000_000, scaled % 100_000_000)
}

fn unassigned_transparent_balance(
    transparent_pool_zat: Option<i64>,
    addressed_balance_zat: i64,
) -> Result<Option<i64>> {
    match transparent_pool_zat {
        Some(transparent_pool_zat) => transparent_pool_zat
            .checked_sub(addressed_balance_zat)
            .filter(|value| *value >= 0)
            .map(Some)
            .ok_or_else(|| {
                ExplorerError::InvalidNodeResponse(
                    "decoded transparent balances exceed the monitored transparent value pool"
                        .to_owned(),
                )
            }),
        None => Ok(None),
    }
}

#[derive(Clone, Debug, FromRow)]
struct NetworkHistoryRow {
    height: i64,
    hash: String,
    block_time: DateTime<Utc>,
    difficulty_text: String,
    spacing_seconds: Option<i64>,
    size_bytes: i64,
    transaction_count: i32,
    chain_supply_zat: Option<i64>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NetworkHistoryView {
    pub as_of_height: Option<u64>,
    pub as_of_hash: Option<String>,
    pub target_spacing_seconds: u32,
    pub points: Vec<NetworkHistoryPoint>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NetworkHistoryPoint {
    pub height: u64,
    pub hash: String,
    pub time: DateTime<Utc>,
    pub difficulty: String,
    pub spacing_seconds: Option<i64>,
    pub size_bytes: i64,
    pub transaction_count: i32,
    pub total_issued: Option<AmountView>,
}

#[derive(Clone, Debug, FromRow)]
struct ValuePoolHistoryRow {
    height: i64,
    hash: String,
    block_time: DateTime<Utc>,
    chain_supply_zat: Option<i64>,
    transparent_chain_value_zat: Option<i64>,
    transparent_value_delta_zat: Option<i64>,
    transparent_monitored: Option<bool>,
    ironwood_chain_value_zat: Option<i64>,
    ironwood_value_delta_zat: Option<i64>,
    ironwood_monitored: Option<bool>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PoolSnapshotView {
    pub id: String,
    pub chain_value: Option<AmountView>,
    pub value_delta: Option<AmountView>,
    pub reported: bool,
    /// Raw compatibility flag from the node; it is not a presence signal.
    pub monitored: Option<bool>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ValuePoolHistoryPoint {
    pub height: u64,
    pub hash: String,
    pub time: DateTime<Utc>,
    pub total_issued: Option<AmountView>,
    pub transparent: PoolSnapshotView,
    pub ironwood: PoolSnapshotView,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ValuePoolHistoryView {
    pub as_of_height: Option<u64>,
    pub as_of_hash: Option<String>,
    pub unexpected_pool_samples: i64,
    pub scope_notice: String,
    pub points: Vec<ValuePoolHistoryPoint>,
}

#[derive(Clone, Debug, FromRow)]
struct MergeMiningStatsRow {
    eligible_child_blocks: i64,
    auxpow_blocks: i64,
    locally_verified_blocks: i64,
    parent_target_verified_blocks: i64,
    canonical_parent_blocks: i64,
    orphaned_parent_blocks: i64,
    not_found_parent_blocks: i64,
    unavailable_parent_blocks: i64,
    disagreement_parent_blocks: i64,
    parent_quorum_agreement_blocks: i64,
    best_chain_witness_blocks: i64,
    fully_verified_blocks: i64,
    locally_verified_without_parent_observation_blocks: i64,
    anomaly_blocks: i64,
    observation_source_count: i64,
    last_verified_at: Option<DateTime<Utc>>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MergeMiningStatsView {
    pub as_of_height: Option<u64>,
    pub as_of_hash: Option<String>,
    pub eligible_child_blocks: i64,
    pub auxpow_blocks: i64,
    pub locally_verified_blocks: i64,
    pub parent_target_verified_blocks: i64,
    pub canonical_parent_blocks: i64,
    pub orphaned_parent_blocks: i64,
    pub not_found_parent_blocks: i64,
    pub unavailable_parent_blocks: i64,
    pub disagreement_parent_blocks: i64,
    pub parent_quorum_agreement_blocks: i64,
    pub best_chain_witness_blocks: i64,
    pub fully_verified_blocks: i64,
    pub locally_verified_without_parent_observation_blocks: i64,
    pub anomaly_blocks: i64,
    pub observation_source_count: i64,
    pub last_verified_at: Option<DateTime<Utc>>,
    pub scope_notice: String,
}

#[derive(Clone, Debug, FromRow)]
struct RichListRow {
    rank: i64,
    address: String,
    balance_zat: i64,
    total_received_zat: String,
    total_sent_zat: String,
    utxo_count: i64,
    transaction_count: i64,
    first_seen_height: Option<i64>,
    last_seen_height: Option<i64>,
    last_seen_at: Option<DateTime<Utc>>,
    funded_address_count: i64,
    addressed_balance_zat: i64,
    top_1_zat: i64,
    top_10_zat: i64,
    top_100_zat: i64,
}

#[derive(Clone, Debug, Default)]
struct RichListTotals {
    funded_address_count: i64,
    addressed_balance_zat: i64,
    top_1_zat: i64,
    top_10_zat: i64,
    top_100_zat: i64,
}

#[derive(Clone, Debug, FromRow)]
struct LatestPoolRow {
    chain_value_zat: Option<i64>,
    monitored: Option<bool>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RichListEntry {
    pub rank: u64,
    pub address: String,
    pub balance: AmountView,
    pub total_received: AmountView,
    pub total_sent: AmountView,
    pub transparent_pool_share_percent: Option<String>,
    pub utxo_count: i64,
    pub transaction_count: i64,
    pub first_seen_height: Option<u64>,
    pub last_seen_height: Option<u64>,
    pub last_seen_at: Option<DateTime<Utc>>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RichListView {
    pub as_of_height: Option<u64>,
    pub as_of_hash: Option<String>,
    pub transparent_pool: Option<AmountView>,
    pub transparent_pool_reported: bool,
    pub transparent_pool_monitored: Option<bool>,
    pub funded_address_count: i64,
    pub addressed_balance: AmountView,
    pub addressless_or_undecoded_balance: Option<AmountView>,
    pub top_1_balance: AmountView,
    pub top_10_balance: AmountView,
    pub top_100_balance: AmountView,
    pub addresses: Vec<RichListEntry>,
    pub scope_notice: String,
}

#[derive(Clone, Debug, FromRow)]
struct AddressStatsRow {
    height: i64,
    block_time: DateTime<Utc>,
    active_addresses: i64,
    new_addresses: i64,
    total_seen_addresses: i64,
}

#[derive(Clone, Debug, FromRow)]
struct CurrentAddressStatsRow {
    funded_address_count: i64,
    seen_address_count: i64,
    addressed_balance_zat: i64,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AddressStatsPoint {
    pub height: u64,
    pub time: DateTime<Utc>,
    pub active_addresses: i64,
    pub new_addresses: i64,
    pub total_seen_addresses: i64,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AddressStatsView {
    pub as_of_height: Option<u64>,
    pub as_of_hash: Option<String>,
    pub funded_address_count: i64,
    pub seen_address_count: i64,
    pub addressed_balance: AmountView,
    pub transparent_pool: Option<AmountView>,
    pub transparent_pool_reported: bool,
    pub transparent_pool_monitored: Option<bool>,
    pub addressless_or_undecoded_balance: Option<AmountView>,
    pub points: Vec<AddressStatsPoint>,
    pub scope_notice: String,
}

const NETWORK_HISTORY_QUERY: &str = "WITH selectors AS MATERIALIZED (
        SELECT c.height, c.block_hash, c.witness_hash
        FROM canonical_chain c
        WHERE c.network_id = $1
          AND ($2::BIGINT IS NULL OR c.height < $2)
        ORDER BY c.height DESC
        LIMIT ($3 + 1)
    ), selected AS (
        SELECT c.height, c.block_hash AS hash, b.block_time,
               b.difficulty_text, b.size_bytes, b.transaction_count,
               b.chain_supply_zat
        FROM selectors c
        JOIN blocks b ON b.block_hash = c.block_hash
    ), history AS (
        SELECT *,
               EXTRACT(EPOCH FROM (
                   block_time - LAG(block_time) OVER (ORDER BY height)
               ))::BIGINT AS spacing_seconds
        FROM selected
    ), recent AS (
        SELECT * FROM history
        ORDER BY height DESC LIMIT $3
    )
    SELECT * FROM recent ORDER BY height";

const VALUE_POOL_HISTORY_QUERY: &str = "WITH selected AS MATERIALIZED (
        SELECT c.height, c.block_hash, c.witness_hash
        FROM canonical_chain c
        WHERE c.network_id = $1
          AND ($2::BIGINT IS NULL OR c.height < $2)
        ORDER BY c.height DESC
        LIMIT $3
    ), history AS (
        SELECT c.height, c.block_hash AS hash, b.block_time, b.chain_supply_zat,
               MAX(p.chain_value_zat) FILTER (WHERE p.pool_id = 'transparent')::BIGINT
                   AS transparent_chain_value_zat,
               MAX(p.value_delta_zat) FILTER (WHERE p.pool_id = 'transparent')::BIGINT
                   AS transparent_value_delta_zat,
               BOOL_OR(p.monitored) FILTER (WHERE p.pool_id = 'transparent')
                   AS transparent_monitored,
               MAX(p.chain_value_zat) FILTER (WHERE p.pool_id = 'ironwood')::BIGINT
                   AS ironwood_chain_value_zat,
               MAX(p.value_delta_zat) FILTER (WHERE p.pool_id = 'ironwood')::BIGINT
                   AS ironwood_value_delta_zat,
               BOOL_OR(p.monitored) FILTER (WHERE p.pool_id = 'ironwood')
                   AS ironwood_monitored
        FROM selected c
        JOIN blocks b ON b.block_hash = c.block_hash
        LEFT JOIN value_pool_snapshots p
          ON p.block_hash = c.block_hash AND p.witness_hash = c.witness_hash
         AND p.pool_id IN ('transparent', 'ironwood')
        GROUP BY c.height, c.block_hash, b.block_time, b.chain_supply_zat
    )
    SELECT * FROM history ORDER BY height";

const MERGE_MINING_STATS_QUERY: &str = "WITH evidence AS (
        SELECT c.height, w.exact_witness_state, w.local_validation_state,
               a.parent_hash_meets_claimed_target, a.parent_lookup_state,
               a.parent_sources_agree, a.verified_at,
               EXISTS (
                   SELECT 1 FROM parent_chain_observations p
                   WHERE p.block_hash = c.block_hash
                     AND p.witness_hash = c.witness_hash
                     AND p.observation_state = 'canonical'
                     AND p.embedded_header_matches IS TRUE
               ) AS has_canonical_parent,
               (SELECT COUNT(DISTINCT p.source_name)
                FROM parent_chain_observations p
                WHERE p.block_hash = c.block_hash
                  AND p.witness_hash = c.witness_hash) AS source_count,
               (a.block_hash IS NOT NULL) AS has_auxpow
        FROM canonical_chain c
        JOIN block_witnesses w
          ON w.block_hash = c.block_hash AND w.witness_hash = c.witness_hash
        LEFT JOIN auxpow_links a
          ON a.block_hash = c.block_hash AND a.witness_hash = c.witness_hash
        WHERE c.network_id = $1
    )
    SELECT
        COUNT(*) FILTER (WHERE height > 0)::BIGINT AS eligible_child_blocks,
        COUNT(*) FILTER (WHERE height > 0 AND has_auxpow)::BIGINT AS auxpow_blocks,
        COUNT(*) FILTER (
            WHERE height > 0 AND local_validation_state = 'auxpow_verified'
        )::BIGINT AS locally_verified_blocks,
        COUNT(*) FILTER (
            WHERE height > 0 AND parent_hash_meets_claimed_target IS TRUE
        )::BIGINT AS parent_target_verified_blocks,
        COUNT(*) FILTER (
            WHERE height > 0 AND has_canonical_parent
        )::BIGINT AS canonical_parent_blocks,
        COUNT(*) FILTER (
            WHERE height > 0 AND parent_lookup_state = 'orphaned'
        )::BIGINT AS orphaned_parent_blocks,
        COUNT(*) FILTER (
            WHERE height > 0 AND parent_lookup_state = 'not_found'
        )::BIGINT AS not_found_parent_blocks,
        COUNT(*) FILTER (
            WHERE height > 0 AND parent_lookup_state IN ('unavailable', 'unknown')
        )::BIGINT AS unavailable_parent_blocks,
        COUNT(*) FILTER (
            WHERE height > 0
              AND has_auxpow
              AND parent_lookup_state <> 'not_configured'
              AND parent_sources_agree IS NOT TRUE
        )::BIGINT AS disagreement_parent_blocks,
        COUNT(*) FILTER (
            WHERE height > 0 AND parent_sources_agree IS TRUE
        )::BIGINT AS parent_quorum_agreement_blocks,
        COUNT(*) FILTER (
            WHERE height > 0 AND exact_witness_state = 'best_chain'
        )::BIGINT AS best_chain_witness_blocks,
        COUNT(*) FILTER (
            WHERE height > 0
              AND has_auxpow
              AND exact_witness_state = 'best_chain'
              AND local_validation_state = 'auxpow_verified'
              AND parent_hash_meets_claimed_target IS TRUE
              AND parent_sources_agree IS TRUE
              AND has_canonical_parent
        )::BIGINT AS fully_verified_blocks,
        COUNT(*) FILTER (
            WHERE height > 0
              AND has_auxpow
              AND exact_witness_state = 'best_chain'
              AND local_validation_state = 'auxpow_verified'
              AND parent_lookup_state = 'not_configured'
        )::BIGINT AS locally_verified_without_parent_observation_blocks,
        COUNT(*) FILTER (
            WHERE height > 0
              AND ((
                    has_auxpow
                AND exact_witness_state = 'best_chain'
                AND local_validation_state = 'auxpow_verified'
                AND parent_hash_meets_claimed_target IS TRUE
                AND parent_sources_agree IS TRUE
                AND has_canonical_parent
              ) OR (
                    has_auxpow
                AND exact_witness_state = 'best_chain'
                AND local_validation_state = 'auxpow_verified'
                AND parent_lookup_state = 'not_configured'
              )) IS NOT TRUE
        )::BIGINT AS anomaly_blocks,
        COALESCE(MAX(source_count), 0)::BIGINT AS observation_source_count,
        MAX(verified_at) FILTER (WHERE height > 0) AS last_verified_at
    FROM evidence";

const RICH_LIST_QUERY: &str = "WITH canonical_tx AS MATERIALIZED (
        SELECT c.height, c.block_hash, b.block_time, bt.tx_position,
               ti.transaction_instance_id, ti.txid, ti.is_coinbase
        FROM canonical_chain c
        JOIN blocks b ON b.block_hash = c.block_hash
        JOIN block_transactions bt
          ON bt.block_hash = c.block_hash AND bt.witness_hash = c.witness_hash
        JOIN transaction_instances ti
          ON ti.transaction_instance_id = bt.transaction_instance_id
        WHERE c.network_id = $1
    ), spent_prevouts AS MATERIALIZED (
        SELECT DISTINCT i.previous_txid, i.previous_output_index
        FROM canonical_tx tx
        JOIN transparent_inputs i ON i.transaction_instance_id = tx.transaction_instance_id
        WHERE i.previous_txid IS NOT NULL
    ), received AS (
        SELECT o.address, SUM(o.value_zat::NUMERIC) AS total_received_zat
        FROM canonical_tx tx
        JOIN transparent_outputs o ON o.transaction_instance_id = tx.transaction_instance_id
        WHERE o.address IS NOT NULL GROUP BY o.address
    ), spent AS (
        SELECT o.address, SUM(o.value_zat::NUMERIC) AS total_sent_zat
        FROM canonical_tx tx
        JOIN transparent_outputs o ON o.transaction_instance_id = tx.transaction_instance_id
        JOIN spent_prevouts s ON s.previous_txid = tx.txid AND s.previous_output_index = o.output_index
        WHERE o.address IS NOT NULL GROUP BY o.address
    ), utxos AS (
        SELECT o.address, SUM(o.value_zat)::BIGINT AS balance_zat, COUNT(*)::BIGINT AS utxo_count
        FROM canonical_tx tx
        JOIN transparent_outputs o ON o.transaction_instance_id = tx.transaction_instance_id
        LEFT JOIN spent_prevouts s ON s.previous_txid = tx.txid AND s.previous_output_index = o.output_index
        WHERE o.address IS NOT NULL AND s.previous_txid IS NULL
        GROUP BY o.address HAVING SUM(o.value_zat) > 0
    ), activity_events AS (
        SELECT o.address, tx.height, tx.block_time, tx.tx_position,
               tx.transaction_instance_id
        FROM canonical_tx tx
        JOIN transparent_outputs o ON o.transaction_instance_id = tx.transaction_instance_id
        WHERE o.address IS NOT NULL
        UNION
        SELECT previous_output.address, spending_tx.height, spending_tx.block_time,
               spending_tx.tx_position,
               spending_tx.transaction_instance_id
        FROM canonical_tx spending_tx
        JOIN transparent_inputs input ON input.transaction_instance_id = spending_tx.transaction_instance_id
        JOIN canonical_tx previous_tx ON previous_tx.txid = input.previous_txid
        JOIN transparent_outputs previous_output
          ON previous_output.transaction_instance_id = previous_tx.transaction_instance_id
         AND previous_output.output_index = input.previous_output_index
        WHERE previous_output.address IS NOT NULL
    ), activity AS (
        SELECT address, COUNT(*)::BIGINT AS transaction_count,
               MIN(height) AS first_seen_height, MAX(height) AS last_seen_height,
               (ARRAY_AGG(
                   block_time
                   ORDER BY height DESC, tx_position DESC, transaction_instance_id DESC
               ))[1] AS last_seen_at
        FROM activity_events GROUP BY address
    ), balances AS (
        SELECT u.address, u.balance_zat, u.utxo_count, r.total_received_zat,
               COALESCE(s.total_sent_zat, 0::NUMERIC) AS total_sent_zat,
               COALESCE(a.transaction_count, 0)::BIGINT AS transaction_count,
               a.first_seen_height, a.last_seen_height, a.last_seen_at
        FROM utxos u JOIN received r ON r.address = u.address
        LEFT JOIN spent s ON s.address = u.address
        LEFT JOIN activity a ON a.address = u.address
    ), ranked AS (
        SELECT *, ROW_NUMBER() OVER (ORDER BY balance_zat DESC, address)::BIGINT AS rank
        FROM balances
    )
    SELECT rank, address, balance_zat, total_received_zat::TEXT AS total_received_zat,
           total_sent_zat::TEXT AS total_sent_zat,
           utxo_count, transaction_count, first_seen_height, last_seen_height, last_seen_at,
           COUNT(*) OVER ()::BIGINT AS funded_address_count,
           COALESCE(SUM(balance_zat) OVER (), 0)::BIGINT AS addressed_balance_zat,
           COALESCE(SUM(balance_zat) FILTER (WHERE rank <= 1) OVER (), 0)::BIGINT AS top_1_zat,
           COALESCE(SUM(balance_zat) FILTER (WHERE rank <= 10) OVER (), 0)::BIGINT AS top_10_zat,
           COALESCE(SUM(balance_zat) FILTER (WHERE rank <= 100) OVER (), 0)::BIGINT AS top_100_zat
    FROM ranked ORDER BY rank LIMIT $2";

const LATEST_TRANSPARENT_POOL_QUERY: &str = "SELECT p.chain_value_zat, p.monitored
     FROM chain_state s
     LEFT JOIN canonical_chain c
       ON c.network_id = s.network_id
      AND c.height = s.indexed_height
      AND c.block_hash = s.indexed_hash
     LEFT JOIN value_pool_snapshots p
       ON p.block_hash = c.block_hash
      AND p.witness_hash = c.witness_hash
      AND p.pool_id = 'transparent'
     WHERE s.network_id = $1";

const ADDRESS_STATS_QUERY: &str =
    "WITH canonical_tx AS MATERIALIZED (
        SELECT c.height, b.block_time, ti.transaction_instance_id, ti.txid
        FROM canonical_chain c
        JOIN blocks b ON b.block_hash = c.block_hash
        JOIN block_transactions bt
          ON bt.block_hash = c.block_hash AND bt.witness_hash = c.witness_hash
        JOIN transaction_instances ti ON ti.transaction_instance_id = bt.transaction_instance_id
        WHERE c.network_id = $1
    ), events AS (
        SELECT o.address, tx.height
        FROM canonical_tx tx
        JOIN transparent_outputs o ON o.transaction_instance_id = tx.transaction_instance_id
        WHERE o.address IS NOT NULL
        UNION
        SELECT previous_output.address, spending_tx.height
        FROM canonical_tx spending_tx
        JOIN transparent_inputs input ON input.transaction_instance_id = spending_tx.transaction_instance_id
        JOIN canonical_tx previous_tx ON previous_tx.txid = input.previous_txid
        JOIN transparent_outputs previous_output
          ON previous_output.transaction_instance_id = previous_tx.transaction_instance_id
         AND previous_output.output_index = input.previous_output_index
        WHERE previous_output.address IS NOT NULL
    ), active AS (
        SELECT height, COUNT(DISTINCT address)::BIGINT AS active_addresses
        FROM events GROUP BY height
    ), first_seen AS (
        SELECT address, MIN(height) AS height FROM events GROUP BY address
    ), new_by_height AS (
        SELECT height, COUNT(*)::BIGINT AS new_addresses FROM first_seen GROUP BY height
    ), metrics AS (
        SELECT c.height, b.block_time,
               COALESCE(a.active_addresses, 0)::BIGINT AS active_addresses,
               COALESCE(n.new_addresses, 0)::BIGINT AS new_addresses,
               SUM(COALESCE(n.new_addresses, 0)) OVER (ORDER BY c.height)::BIGINT
                   AS total_seen_addresses
        FROM canonical_chain c
        JOIN blocks b ON b.block_hash = c.block_hash
        LEFT JOIN active a ON a.height = c.height
        LEFT JOIN new_by_height n ON n.height = c.height
        WHERE c.network_id = $1
    ), recent AS (
        SELECT * FROM metrics
        WHERE ($2::BIGINT IS NULL OR height < $2)
        ORDER BY height DESC LIMIT $3
    )
    SELECT * FROM recent ORDER BY height";

const CURRENT_ADDRESS_STATS_QUERY: &str = "WITH canonical_tx AS MATERIALIZED (
        SELECT ti.transaction_instance_id, ti.txid
        FROM canonical_chain c
        JOIN block_transactions bt
          ON bt.block_hash = c.block_hash AND bt.witness_hash = c.witness_hash
        JOIN transaction_instances ti ON ti.transaction_instance_id = bt.transaction_instance_id
        WHERE c.network_id = $1
    ), spent_prevouts AS MATERIALIZED (
        SELECT DISTINCT i.previous_txid, i.previous_output_index
        FROM canonical_tx tx
        JOIN transparent_inputs i ON i.transaction_instance_id = tx.transaction_instance_id
        WHERE i.previous_txid IS NOT NULL
    ), seen AS (
        SELECT DISTINCT o.address
        FROM canonical_tx tx
        JOIN transparent_outputs o ON o.transaction_instance_id = tx.transaction_instance_id
        WHERE o.address IS NOT NULL
    ), balances AS (
        SELECT o.address, SUM(o.value_zat)::BIGINT AS balance_zat
        FROM canonical_tx tx
        JOIN transparent_outputs o ON o.transaction_instance_id = tx.transaction_instance_id
        LEFT JOIN spent_prevouts s
          ON s.previous_txid = tx.txid AND s.previous_output_index = o.output_index
        WHERE o.address IS NOT NULL AND s.previous_txid IS NULL
        GROUP BY o.address HAVING SUM(o.value_zat) > 0
    )
    SELECT (SELECT COUNT(*) FROM balances)::BIGINT AS funded_address_count,
           (SELECT COUNT(*) FROM seen)::BIGINT AS seen_address_count,
           COALESCE((SELECT SUM(balance_zat) FROM balances), 0)::BIGINT AS addressed_balance_zat";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exact_percent_does_not_use_floating_point() {
        assert_eq!(exact_percent(1, 3), "33.33333333");
        assert_eq!(exact_percent(625_000_000, 30_000_000_000), "2.08333333");
        assert_eq!(exact_percent(0, 1), "0.00000000");
    }

    #[test]
    fn history_limits_are_bounded_for_public_queries() {
        assert_eq!(history_limit(None), DEFAULT_HISTORY_LIMIT);
        assert_eq!(history_limit(Some(1)), 2);
        assert_eq!(history_limit(Some(u16::MAX)), MAX_HISTORY_LIMIT);
    }

    #[test]
    fn unassigned_balance_uses_exact_reported_pool_data_including_zero() {
        assert_eq!(
            unassigned_transparent_balance(Some(100), 75).expect("consistent pool data"),
            Some(25)
        );
        assert_eq!(
            unassigned_transparent_balance(Some(0), 0).expect("reported zero pool"),
            Some(0)
        );
        assert_eq!(unassigned_transparent_balance(None, 75).unwrap(), None);
        assert!(unassigned_transparent_balance(Some(50), 75).is_err());
    }
}
