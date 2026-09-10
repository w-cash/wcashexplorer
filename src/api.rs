//! Read-only, cursor-paginated explorer HTTP API.

use std::sync::Arc;

use axum::{
    Json, Router,
    body::Body,
    extract::{DefaultBodyLimit, Path, Query, State},
    http::{HeaderName, HeaderValue, Request, header},
    routing::get,
};
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sqlx::FromRow;
use tower::limit::ConcurrencyLimitLayer;
use tower_http::{
    catch_panic::CatchPanicLayer,
    compression::CompressionLayer,
    request_id::{MakeRequestUuid, PropagateRequestIdLayer, SetRequestIdLayer},
    sensitive_headers::SetSensitiveRequestHeadersLayer,
    set_header::SetResponseHeaderLayer,
    trace::TraceLayer,
};
use uuid::Uuid;

use crate::{
    config::NetworkConfig,
    db::Database,
    error::{ExplorerError, Result},
    models::{ApiEnvelope, PageMeta},
};

const DEFAULT_PAGE_SIZE: u16 = 20;
const MAX_PAGE_SIZE: u16 = 100;
const READY_MAX_AGE_SECONDS: i64 = 30;

/// Immutable application state shared by API handlers.
#[derive(Clone)]
pub struct AppState {
    pub database: Database,
    pub network: NetworkConfig,
    pub started_at: DateTime<Utc>,
}

impl AppState {
    pub fn new(database: Database, network: NetworkConfig) -> Self {
        Self {
            database,
            network,
            started_at: Utc::now(),
        }
    }
}

/// Builds the public read-only API. Node RPC is intentionally never proxied.
pub fn router(state: AppState) -> Router {
    let request_id_header = HeaderName::from_static("x-request-id");
    Router::new()
        .route("/health/live", get(live))
        .route("/health/ready", get(ready))
        .route("/api/v1/status", get(status))
        .route("/api/v1/stats", get(status))
        .route("/api/v1/blocks", get(blocks))
        .route("/api/v1/blocks/{id}", get(block))
        .route("/api/v1/blocks/{id}/raw", get(raw_block))
        .route("/api/v1/blocks/{id}/auxpow", get(block_auxpow))
        .route("/api/v1/transactions", get(transactions))
        .route("/api/v1/transactions/{txid}", get(transaction))
        .route("/api/v1/addresses/{address}", get(address))
        .route("/api/v1/reorgs", get(reorgs))
        .route("/api/v1/search", get(search))
        .route("/api/openapi.json", get(openapi))
        .with_state(Arc::new(state))
        .layer(DefaultBodyLimit::max(16 * 1024))
        .layer(ConcurrencyLimitLayer::new(256))
        .layer(CompressionLayer::new())
        .layer(SetResponseHeaderLayer::if_not_present(
            header::CONTENT_SECURITY_POLICY,
            HeaderValue::from_static(
                "default-src 'none'; frame-ancestors 'none'; base-uri 'none'; form-action 'none'",
            ),
        ))
        .layer(SetResponseHeaderLayer::if_not_present(
            header::X_CONTENT_TYPE_OPTIONS,
            HeaderValue::from_static("nosniff"),
        ))
        .layer(SetResponseHeaderLayer::if_not_present(
            header::REFERRER_POLICY,
            HeaderValue::from_static("no-referrer"),
        ))
        .layer(SetSensitiveRequestHeadersLayer::new(std::iter::once(
            header::AUTHORIZATION,
        )))
        .layer(PropagateRequestIdLayer::new(request_id_header.clone()))
        .layer(SetRequestIdLayer::new(request_id_header, MakeRequestUuid))
        .layer(CatchPanicLayer::new())
        .layer(
            TraceLayer::new_for_http().make_span_with(|request: &Request<Body>| {
                tracing::info_span!(
                    "http_request",
                    method = %request.method(),
                    path = %request.uri().path(),
                )
            }),
        )
}

async fn live() -> Json<Value> {
    Json(json!({"status": "live", "version": env!("CARGO_PKG_VERSION")}))
}

async fn ready(State(state): State<Arc<AppState>>) -> Result<Json<Value>> {
    let row = chain_state(&state).await?;
    let lag = row
        .node_height
        .zip(row.indexed_height)
        .map(|(node, indexed)| node.saturating_sub(indexed));
    let age = (Utc::now() - row.updated_at).num_seconds().max(0);
    let tips_present = row.indexed_height.is_some()
        && row.indexed_hash.is_some()
        && row.node_height.is_some()
        && row.node_hash.is_some();
    let same_height_hash_matches = row.indexed_height != row.node_height
        || row.indexed_hash.as_deref() == row.node_hash.as_deref();
    let canonical_tip_exists: bool = match (row.indexed_height, row.indexed_hash.as_deref()) {
        (Some(height), Some(hash)) => {
            sqlx::query_scalar(
                "SELECT EXISTS(
                    SELECT 1 FROM canonical_chain
                    WHERE network_id = $1 AND height = $2 AND block_hash = $3
                )",
            )
            .bind(&state.network.id)
            .bind(height)
            .bind(hash)
            .fetch_one(state.database.pool())
            .await?
        }
        _ => false,
    };
    if row.status != "ready"
        || lag.is_none_or(|blocks| blocks > 1)
        || age > READY_MAX_AGE_SECONDS
        || !tips_present
        || !same_height_hash_matches
        || !canonical_tip_exists
    {
        return Err(ExplorerError::NotReady(format!(
            "indexer status is {}, lag is {} block(s), heartbeat age is {age}s, and canonical tip integrity is {}",
            row.status,
            lag.map_or_else(|| "unknown".to_owned(), |blocks| blocks.to_string()),
            if canonical_tip_exists && same_height_hash_matches {
                "valid"
            } else {
                "invalid"
            }
        )));
    }
    Ok(Json(json!({
        "status": "ready",
        "network": state.network.id,
        "indexedHeight": row.indexed_height,
        "nodeHeight": row.node_height,
        "updatedAt": row.updated_at,
    })))
}

async fn status(State(state): State<Arc<AppState>>) -> Result<Json<ApiEnvelope<StatusView>>> {
    let chain = chain_state(&state).await?;
    let latest = sqlx::query_as::<_, LatestChainRow>(
        "SELECT b.difficulty_text, b.chain_supply_zat, b.block_time,
                (SELECT (
                    EXTRACT(EPOCH FROM MAX(block_time) - MIN(block_time))
                    / NULLIF(MAX(height) - MIN(height), 0)
                 )::DOUBLE PRECISION
                 FROM (
                    SELECT c2.height, b2.block_time
                    FROM canonical_chain c2
                    JOIN blocks b2 ON b2.block_hash = c2.block_hash
                    WHERE c2.network_id = $1
                    ORDER BY c2.height DESC LIMIT 101
                 ) recent) AS observed_spacing_seconds
         FROM canonical_chain c
         JOIN blocks b ON b.block_hash = c.block_hash
         WHERE c.network_id = $1
         ORDER BY c.height DESC LIMIT 1",
    )
    .bind(&state.network.id)
    .fetch_optional(state.database.pool())
    .await?;
    let indexed_height = optional_u64(chain.indexed_height, "indexed height")?;
    let node_height = optional_u64(chain.node_height, "node height")?;
    let data = StatusView {
        service: "wcashexplorer".to_owned(),
        version: env!("CARGO_PKG_VERSION").to_owned(),
        network: state.network.id.clone(),
        network_name: state.network.display_name.clone(),
        symbol: state.network.symbol.clone(),
        status: chain.status.clone(),
        indexed_height,
        indexed_hash: chain.indexed_hash.clone(),
        node_height,
        node_hash: chain.node_hash.clone(),
        lag_blocks: chain
            .node_height
            .zip(chain.indexed_height)
            .map(|(node, indexed)| node.saturating_sub(indexed)),
        updated_at: chain.updated_at,
        uptime_seconds: (Utc::now() - state.started_at).num_seconds().max(0),
        difficulty: latest.as_ref().map(|row| row.difficulty_text.clone()),
        observed_spacing_seconds: latest.as_ref().and_then(|row| row.observed_spacing_seconds),
        target_spacing_seconds: state.network.target_spacing_seconds,
        total_issued: amount(
            latest
                .as_ref()
                .and_then(|row| row.chain_supply_zat)
                .unwrap_or(0),
            &state.network,
        ),
        max_supply: amount(state.network.max_supply_zat, &state.network),
        initial_subsidy: amount(state.network.initial_subsidy_zat, &state.network),
        halving_interval: state.network.halving_interval,
        next_halving_height: next_halving_height(indexed_height.unwrap_or(0), &state.network),
        coinbase_maturity: state.network.coinbase_maturity,
        latest_block_time: latest.map(|row| row.block_time),
        privacy_notice:
            "Shielded senders, recipients, per-note amounts, and memos are not public chain data."
                .to_owned(),
    };
    Ok(Json(envelope(&state, data, None, &chain)))
}

async fn blocks(
    State(state): State<Arc<AppState>>,
    Query(query): Query<PageQuery>,
) -> Result<Json<ApiEnvelope<Vec<BlockSummary>>>> {
    let chain = chain_state(&state).await?;
    let before = query.cursor.as_deref().map(decode_cursor).transpose()?;
    let limit = bounded_limit(query.limit);
    let rows = sqlx::query_as::<_, BlockSummaryRow>(BLOCK_SUMMARY_QUERY)
        .bind(&state.network.id)
        .bind(before)
        .bind(i64::from(limit) + 1)
        .fetch_all(state.database.pool())
        .await?;
    let has_more = rows.len() > usize::from(limit);
    let data = rows
        .into_iter()
        .take(usize::from(limit))
        .map(|row| BlockSummary::from_row(row, &state.network))
        .collect::<Result<Vec<_>>>()?;
    let next_cursor = has_more
        .then(|| data.last().map(|block| encode_cursor(block.height)))
        .flatten();
    Ok(Json(envelope(&state, data, next_cursor, &chain)))
}

async fn block(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Result<Json<ApiEnvelope<BlockDetail>>> {
    let chain = chain_state(&state).await?;
    let row = find_block(&state, &id).await?;
    let summary = BlockSummary::from_row(row.summary, &state.network)?;
    let pools = sqlx::query_as::<_, ValuePoolRow>(
        "SELECT pool_id, chain_value_zat, value_delta_zat, monitored
         FROM value_pool_snapshots
         WHERE block_hash = $1 AND witness_hash = $2
         ORDER BY pool_id",
    )
    .bind(&summary.hash)
    .bind(&summary.witness_hash)
    .fetch_all(state.database.pool())
    .await?
    .into_iter()
    .map(|pool| ValuePoolView {
        id: pool.pool_id,
        chain_value: pool
            .chain_value_zat
            .map(|value| amount(value, &state.network)),
        value_delta: pool
            .value_delta_zat
            .map(|value| amount(value, &state.network)),
        monitored: pool.monitored,
    })
    .collect();
    let txs = transaction_rows_for_block(&state, &summary.hash, &summary.witness_hash).await?;
    let auxpow = load_auxpow(&state, &summary.hash, &summary.witness_hash).await?;
    let data = BlockDetail {
        summary,
        previous_block_hash: row.previous_block_hash,
        next_block_hash: row.next_block_hash,
        merkle_root: row.merkle_root,
        block_commitments: row.block_commitments,
        nonce: row.nonce,
        value_pools: pools,
        transactions: txs,
        auxpow,
    };
    Ok(Json(envelope(&state, data, None, &chain)))
}

async fn raw_block(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Result<Json<ApiEnvelope<RawBlockView>>> {
    let chain = chain_state(&state).await?;
    let row = find_block(&state, &id).await?;
    let raw: Vec<u8> = sqlx::query_scalar(
        "SELECT raw_block FROM block_witnesses WHERE block_hash = $1 AND witness_hash = $2",
    )
    .bind(&row.summary.hash)
    .bind(&row.summary.witness_hash)
    .fetch_one(state.database.pool())
    .await?;
    let data = RawBlockView {
        block_hash: row.summary.hash,
        witness_hash: row.summary.witness_hash,
        encoding: "hex".to_owned(),
        data: hex::encode(raw),
    };
    Ok(Json(envelope(&state, data, None, &chain)))
}

async fn block_auxpow(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Result<Json<ApiEnvelope<Option<AuxPowView>>>> {
    let chain = chain_state(&state).await?;
    let row = find_block(&state, &id).await?;
    let data = load_auxpow(&state, &row.summary.hash, &row.summary.witness_hash).await?;
    Ok(Json(envelope(&state, data, None, &chain)))
}

async fn transactions(
    State(state): State<Arc<AppState>>,
    Query(query): Query<PageQuery>,
) -> Result<Json<ApiEnvelope<Vec<TransactionSummary>>>> {
    let chain = chain_state(&state).await?;
    let before = query.cursor.as_deref().map(decode_tx_cursor).transpose()?;
    let limit = bounded_limit(query.limit);
    let rows = sqlx::query_as::<_, TransactionSummaryRow>(TRANSACTION_SUMMARY_QUERY)
        .bind(&state.network.id)
        .bind(
            before
                .map(|cursor| to_i64(cursor.0, "transaction cursor height"))
                .transpose()?,
        )
        .bind(before.map_or(i32::MAX, |cursor| cursor.1))
        .bind(i64::from(limit) + 1)
        .fetch_all(state.database.pool())
        .await?;
    let has_more = rows.len() > usize::from(limit);
    let data = rows
        .into_iter()
        .take(usize::from(limit))
        .map(|row| TransactionSummary::from_row(row, &state.network))
        .collect::<Result<Vec<_>>>()?;
    let next_cursor = has_more
        .then(|| {
            data.last()
                .map(|tx| encode_tx_cursor(tx.block_height, tx.position))
        })
        .flatten();
    Ok(Json(envelope(&state, data, next_cursor, &chain)))
}

async fn transaction(
    State(state): State<Arc<AppState>>,
    Path(txid): Path<String>,
    Query(query): Query<TransactionQuery>,
) -> Result<Json<ApiEnvelope<TransactionDetail>>> {
    ensure_hash("transaction ID", &txid)?;
    if let Some(block_hash) = &query.block {
        ensure_hash("block hash", block_hash)?;
    }
    let chain = chain_state(&state).await?;
    let row = sqlx::query_as::<_, TransactionSummaryRow>(TRANSACTION_DETAIL_QUERY)
        .bind(&state.network.id)
        .bind(&txid)
        .bind(query.block.as_deref())
        .fetch_optional(state.database.pool())
        .await?
        .ok_or(ExplorerError::NotFound)?;
    let summary = TransactionSummary::from_row(row, &state.network)?;
    let instance_id: i64 = sqlx::query_scalar(
        "SELECT transaction_instance_id FROM transaction_instances
         WHERE txid = $1 AND auth_digest = $2",
    )
    .bind(&summary.txid)
    .bind(&summary.auth_digest)
    .fetch_one(state.database.pool())
    .await?;
    let inputs = sqlx::query_as::<_, InputView>(
        "SELECT input_index, previous_txid, previous_output_index, coinbase_data, sequence
         FROM transparent_inputs WHERE transaction_instance_id = $1 ORDER BY input_index",
    )
    .bind(instance_id)
    .fetch_all(state.database.pool())
    .await?;
    let outputs = sqlx::query_as::<_, OutputRow>(
        "SELECT output_index, value_zat, address, script_type, script_hex
         FROM transparent_outputs WHERE transaction_instance_id = $1 ORDER BY output_index",
    )
    .bind(instance_id)
    .fetch_all(state.database.pool())
    .await?
    .into_iter()
    .map(|row| OutputView {
        index: row.output_index,
        value: amount(row.value_zat, &state.network),
        address: row.address,
        script_type: row.script_type,
        script_hex: row.script_hex,
    })
    .collect();
    let raw_rpc: Value = sqlx::query_scalar(
        "SELECT raw_rpc FROM transaction_instances WHERE transaction_instance_id = $1",
    )
    .bind(instance_id)
    .fetch_one(state.database.pool())
    .await?;
    let data = TransactionDetail {
        summary,
        inputs,
        outputs,
        raw_rpc,
        privacy_notice:
            "Shielded senders, recipients, note amounts, and memos are not visible on-chain."
                .to_owned(),
    };
    Ok(Json(envelope(&state, data, None, &chain)))
}

async fn address(
    State(state): State<Arc<AppState>>,
    Path(address): Path<String>,
) -> Result<Json<ApiEnvelope<AddressView>>> {
    validate_address(&address)?;
    let chain = chain_state(&state).await?;
    let tip = chain.indexed_height.unwrap_or(0);
    let balances = sqlx::query_as::<_, AddressBalanceRow>(ADDRESS_BALANCE_QUERY)
        .bind(&state.network.id)
        .bind(&address)
        .bind(tip)
        .bind(i64::from(state.network.coinbase_maturity))
        .fetch_one(state.database.pool())
        .await?;
    if balances.total_received_zat == 0 && balances.utxo_count == 0 {
        let exists: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM transparent_outputs WHERE address = $1)",
        )
        .bind(&address)
        .fetch_one(state.database.pool())
        .await?;
        if !exists {
            return Err(ExplorerError::NotFound);
        }
    }
    let activity = sqlx::query_as::<_, AddressActivityRow>(
        "SELECT c.height AS block_height, c.block_hash, b.block_time, ti.txid,
                ti.auth_digest,
                COALESCE(SUM(o.value_zat), 0)::BIGINT AS received_zat
         FROM transparent_outputs o
         JOIN transaction_instances ti ON ti.transaction_instance_id = o.transaction_instance_id
         JOIN block_transactions bt ON bt.transaction_instance_id = ti.transaction_instance_id
         JOIN canonical_chain c ON c.block_hash = bt.block_hash AND c.witness_hash = bt.witness_hash
         JOIN blocks b ON b.block_hash = c.block_hash
         WHERE c.network_id = $1 AND o.address = $2
         GROUP BY c.height, c.block_hash, b.block_time, ti.txid, ti.auth_digest
         ORDER BY c.height DESC LIMIT 100",
    )
    .bind(&state.network.id)
    .bind(&address)
    .fetch_all(state.database.pool())
    .await?
    .into_iter()
    .map(|row| AddressActivity {
        txid: row.txid,
        auth_digest: row.auth_digest,
        block_height: u64::try_from(row.block_height).unwrap_or(0),
        block_hash: row.block_hash,
        block_time: row.block_time,
        received: amount(row.received_zat, &state.network),
    })
    .collect();
    let data = AddressView {
        address,
        address_type: "transparent".to_owned(),
        total_received: amount(balances.total_received_zat, &state.network),
        unspent: amount(balances.unspent_zat, &state.network),
        immature_coinbase: amount(balances.immature_coinbase_zat, &state.network),
        mature_coinbase_must_shield: amount(balances.mature_coinbase_zat, &state.network),
        utxo_count: balances.utxo_count,
        mined_output_count: balances.mined_output_count,
        coinbase_maturity: state.network.coinbase_maturity,
        activity,
        scope_notice:
            "This page shows transparent activity only; shielded balances are not visible."
                .to_owned(),
    };
    Ok(Json(envelope(&state, data, None, &chain)))
}

async fn reorgs(State(state): State<Arc<AppState>>) -> Result<Json<ApiEnvelope<Vec<ReorgView>>>> {
    let chain = chain_state(&state).await?;
    let rows = sqlx::query_as::<_, ReorgView>(
        "SELECT event_id, old_tip_height, old_tip_hash, common_ancestor_height,
                common_ancestor_hash, detected_at, completed_at
         FROM reorg_events WHERE network_id = $1
         ORDER BY detected_at DESC LIMIT 100",
    )
    .bind(&state.network.id)
    .fetch_all(state.database.pool())
    .await?;
    Ok(Json(envelope(&state, rows, None, &chain)))
}

async fn search(
    State(state): State<Arc<AppState>>,
    Query(query): Query<SearchQuery>,
) -> Result<Json<ApiEnvelope<SearchResult>>> {
    let needle = query.q.trim();
    if needle.is_empty() || needle.len() > 256 {
        return Err(ExplorerError::InvalidRequest(
            "search query must contain between 1 and 256 characters".to_owned(),
        ));
    }
    let result = if let Ok(height) = needle.parse::<u64>() {
        let exists: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM canonical_chain WHERE network_id = $1 AND height = $2)",
        )
        .bind(&state.network.id)
        .bind(to_i64(height, "height")?)
        .fetch_one(state.database.pool())
        .await?;
        exists.then(|| SearchResult::new("block", needle, format!("/block/{needle}")))
    } else if is_hash(needle) {
        let block_exists: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM blocks WHERE network_id = $1 AND block_hash = $2)",
        )
        .bind(&state.network.id)
        .bind(needle)
        .fetch_one(state.database.pool())
        .await?;
        if block_exists {
            Some(SearchResult::new(
                "block",
                needle,
                format!("/block/{needle}"),
            ))
        } else {
            let tx_exists: bool = sqlx::query_scalar(
                "SELECT EXISTS(SELECT 1 FROM transaction_instances WHERE txid = $1)",
            )
            .bind(needle)
            .fetch_one(state.database.pool())
            .await?;
            tx_exists.then(|| SearchResult::new("transaction", needle, format!("/tx/{needle}")))
        }
    } else {
        validate_address(needle)?;
        let exists: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM transparent_outputs WHERE address = $1)",
        )
        .bind(needle)
        .fetch_one(state.database.pool())
        .await?;
        exists.then(|| SearchResult::new("address", needle, format!("/address/{needle}")))
    }
    .ok_or(ExplorerError::NotFound)?;
    let chain = chain_state(&state).await?;
    Ok(Json(envelope(&state, result, None, &chain)))
}

async fn openapi() -> Json<Value> {
    Json(openapi_document())
}

#[allow(clippy::too_many_lines)]
fn openapi_document() -> Value {
    json!({
        "openapi": "3.1.0",
        "jsonSchemaDialect": "https://json-schema.org/draft/2020-12/schema",
        "info": {
            "title": "WcashExplorer API",
            "version": env!("CARGO_PKG_VERSION"),
            "description": "Read-only indexed Wcash chain data. Amounts are returned both as exact atomic-unit strings and fixed-point decimal strings. Shielded senders, recipients, per-note amounts, and memos are not public chain data.",
            "license": {"name": "MIT OR Apache-2.0", "identifier": "MIT OR Apache-2.0"}
        },
        "servers": [{"url": "/", "description": "Current WcashExplorer origin"}],
        "security": [],
        "tags": [
            {"name": "Health", "description": "Process liveness and indexed-chain readiness"},
            {"name": "Network", "description": "Wcash network and indexer state"},
            {"name": "Blocks", "description": "Canonical Wcash blocks and exact AuxPoW evidence"},
            {"name": "Transactions", "description": "Canonical transaction instances"},
            {"name": "Addresses", "description": "Public transparent-address activity only"},
            {"name": "Chain", "description": "Canonical-chain search and reorganization history"},
            {"name": "Documentation", "description": "Machine-readable API description"}
        ],
        "paths": {
            "/health/live": {
                "get": {
                    "tags": ["Health"],
                    "summary": "Process liveness",
                    "operationId": "getLiveness",
                    "responses": {
                        "200": success_response("The HTTP process is live.", schema_ref("LiveResponse"))
                    }
                }
            },
            "/health/ready": {
                "get": {
                    "tags": ["Health"],
                    "summary": "Indexed-chain readiness",
                    "description": "Ready only when the index heartbeat is fresh, lag is at most one block, node and index tips agree at equal heights, and the indexed tip exists in the canonical-chain table.",
                    "operationId": "getReadiness",
                    "responses": {
                        "200": success_response("The explorer is ready to serve indexed data.", schema_ref("ReadyResponse")),
                        "500": response_ref("InternalError"),
                        "503": response_ref("NotReady")
                    }
                }
            },
            "/api/v1/status": {
                "get": status_operation("getStatus")
            },
            "/api/v1/stats": {
                "get": status_operation("getStats")
            },
            "/api/v1/blocks": {
                "get": {
                    "tags": ["Blocks"],
                    "summary": "List canonical blocks",
                    "description": "Returns canonical blocks newest first. Pass nextCursor from the response meta object to continue.",
                    "operationId": "listBlocks",
                    "parameters": pagination_parameters(),
                    "responses": {
                        "200": success_response("A page of canonical blocks.", schema_ref("BlockListEnvelope")),
                        "400": response_ref("BadRequest"),
                        "500": response_ref("InternalError")
                    }
                }
            },
            "/api/v1/blocks/{id}": {
                "get": {
                    "tags": ["Blocks"],
                    "summary": "Get a canonical block",
                    "description": "The identifier can be a decimal block height or a 64-character hexadecimal block hash.",
                    "operationId": "getBlock",
                    "parameters": [parameter_ref("BlockId")],
                    "responses": {
                        "200": success_response("The canonical block and its indexed transactions and proof evidence.", schema_ref("BlockDetailEnvelope")),
                        "400": response_ref("BadRequest"),
                        "404": response_ref("NotFound"),
                        "500": response_ref("InternalError")
                    }
                }
            },
            "/api/v1/blocks/{id}/raw": {
                "get": {
                    "tags": ["Blocks"],
                    "summary": "Get an exact raw block witness",
                    "description": "Returns the exact indexed block serialization as lowercase hexadecimal. The witness hash identifies the full serialization, including AuxPoW witness data.",
                    "operationId": "getRawBlock",
                    "parameters": [parameter_ref("BlockId")],
                    "responses": {
                        "200": success_response("The exact raw block witness.", schema_ref("RawBlockEnvelope")),
                        "400": response_ref("BadRequest"),
                        "404": response_ref("NotFound"),
                        "500": response_ref("InternalError")
                    }
                }
            },
            "/api/v1/blocks/{id}/auxpow": {
                "get": {
                    "tags": ["Blocks"],
                    "summary": "Get exact AuxPoW evidence",
                    "description": "Separates local Wcash AuxPoW validation from observations of the committed parent block on configured Zcash nodes. Genesis has no AuxPoW and returns null data.",
                    "operationId": "getBlockAuxPow",
                    "parameters": [parameter_ref("BlockId")],
                    "responses": {
                        "200": success_response("AuxPoW evidence, or null for a block without AuxPoW.", schema_ref("AuxPowEnvelope")),
                        "400": response_ref("BadRequest"),
                        "404": response_ref("NotFound"),
                        "500": response_ref("InternalError")
                    }
                }
            },
            "/api/v1/transactions": {
                "get": {
                    "tags": ["Transactions"],
                    "summary": "List canonical transactions",
                    "description": "Returns transaction instances newest first. A Zcash-family transaction is identified by txid and authDigest and is linked to its containing block.",
                    "operationId": "listTransactions",
                    "parameters": pagination_parameters(),
                    "responses": {
                        "200": success_response("A page of canonical transaction instances.", schema_ref("TransactionListEnvelope")),
                        "400": response_ref("BadRequest"),
                        "500": response_ref("InternalError")
                    }
                }
            },
            "/api/v1/transactions/{txid}": {
                "get": {
                    "tags": ["Transactions"],
                    "summary": "Get a canonical transaction instance",
                    "description": "When the same txid is present in more than one indexed block context, block selects the exact canonical instance.",
                    "operationId": "getTransaction",
                    "parameters": [parameter_ref("TransactionId"), parameter_ref("BlockContext")],
                    "responses": {
                        "200": success_response("The transaction instance and its public data.", schema_ref("TransactionDetailEnvelope")),
                        "400": response_ref("BadRequest"),
                        "404": response_ref("NotFound"),
                        "500": response_ref("InternalError")
                    }
                }
            },
            "/api/v1/addresses/{address}": {
                "get": {
                    "tags": ["Addresses"],
                    "summary": "Get transparent-address activity",
                    "description": "Covers transparent outputs only. Shielded balances and activity are not visible.",
                    "operationId": "getTransparentAddress",
                    "parameters": [parameter_ref("Address")],
                    "responses": {
                        "200": success_response("Public transparent-address balances and recent receiving activity.", schema_ref("AddressEnvelope")),
                        "400": response_ref("BadRequest"),
                        "404": response_ref("NotFound"),
                        "500": response_ref("InternalError")
                    }
                }
            },
            "/api/v1/reorgs": {
                "get": {
                    "tags": ["Chain"],
                    "summary": "List detected chain reorganizations",
                    "operationId": "listReorganizations",
                    "responses": {
                        "200": success_response("Up to 100 recent reorganization events.", schema_ref("ReorgListEnvelope")),
                        "500": response_ref("InternalError")
                    }
                }
            },
            "/api/v1/search": {
                "get": {
                    "tags": ["Chain"],
                    "summary": "Resolve an exact chain identifier",
                    "description": "Resolves a decimal block height, block hash, transaction ID, or indexed transparent address to its explorer route.",
                    "operationId": "searchChain",
                    "parameters": [parameter_ref("SearchQuery")],
                    "responses": {
                        "200": success_response("The resolved explorer route.", schema_ref("SearchEnvelope")),
                        "400": response_ref("BadRequest"),
                        "404": response_ref("NotFound"),
                        "500": response_ref("InternalError")
                    }
                }
            },
            "/api/openapi.json": {
                "get": {
                    "tags": ["Documentation"],
                    "summary": "Get this OpenAPI document",
                    "operationId": "getOpenApiDocument",
                    "responses": {
                        "200": success_response("The OpenAPI 3.1 document.", schema_ref("OpenApiDocument"))
                    }
                }
            }
        },
        "components": {
            "parameters": {
                "BlockId": {
                    "name": "id", "in": "path", "required": true,
                    "description": "Decimal block height or 64-character hexadecimal block hash.",
                    "schema": {"type": "string", "minLength": 1, "maxLength": 64},
                    "examples": {"height": {"value": "1"}, "hash": {"value": "79cdcea38a54f99a59adddf124490a65de499e5677e693caa14dcf24b5f96007"}}
                },
                "TransactionId": {
                    "name": "txid", "in": "path", "required": true,
                    "description": "A 32-byte transaction ID encoded as 64 hexadecimal characters.",
                    "schema": {"$ref": "#/components/schemas/Hash"}
                },
                "Address": {
                    "name": "address", "in": "path", "required": true,
                    "description": "A publicly indexed transparent Wcash address.",
                    "schema": {"type": "string", "minLength": 20, "maxLength": 256, "pattern": "^[A-Za-z0-9_-]+$"}
                },
                "BlockContext": {
                    "name": "block", "in": "query", "required": false,
                    "description": "Containing block hash used to select an exact transaction instance.",
                    "schema": {"$ref": "#/components/schemas/Hash"}
                },
                "SearchQuery": {
                    "name": "q", "in": "query", "required": true,
                    "description": "Exact height, hash, transaction ID, or transparent address.",
                    "schema": {"type": "string", "minLength": 1, "maxLength": 256}
                },
                "Cursor": {
                    "name": "cursor", "in": "query", "required": false,
                    "description": "Opaque URL-safe cursor returned as meta.nextCursor by the preceding page.",
                    "schema": {"type": "string", "minLength": 1}
                },
                "Limit": {
                    "name": "limit", "in": "query", "required": false,
                    "description": "Requested page size. The server defaults to 20 and clamps values to 1 through 100.",
                    "schema": {"type": "integer", "format": "int32", "minimum": 1, "maximum": 100, "default": 20}
                }
            },
            "responses": {
                "BadRequest": problem_response("The request parameters are invalid."),
                "NotFound": problem_response("The requested explorer resource does not exist."),
                "NotReady": problem_response("The explorer has not reached a safe serving state."),
                "InternalError": problem_response("The explorer could not complete the request.")
            },
            "schemas": openapi_schemas()
        }
    })
}

fn status_operation(operation_id: &str) -> Value {
    json!({
        "tags": ["Network"],
        "summary": "Get indexer and network status",
        "operationId": operation_id,
        "responses": {
            "200": success_response("Current Wcash network, issuance, and indexer state.", schema_ref("StatusEnvelope")),
            "500": response_ref("InternalError")
        }
    })
}

fn pagination_parameters() -> Vec<Value> {
    vec![parameter_ref("Cursor"), parameter_ref("Limit")]
}

fn parameter_ref(name: &str) -> Value {
    json!({"$ref": format!("#/components/parameters/{name}")})
}

fn response_ref(name: &str) -> Value {
    json!({"$ref": format!("#/components/responses/{name}")})
}

fn schema_ref(name: &str) -> Value {
    json!({"$ref": format!("#/components/schemas/{name}")})
}

fn success_response(description: &str, schema: impl Into<Value>) -> Value {
    let schema = schema.into();
    json!({
        "description": description,
        "content": {"application/json": {"schema": schema}}
    })
}

fn problem_response(description: &str) -> Value {
    json!({
        "description": description,
        "content": {"application/problem+json": {"schema": schema_ref("ProblemDetails")}}
    })
}

#[allow(clippy::too_many_lines)]
fn openapi_schemas() -> Value {
    let mut schemas = serde_json::Map::new();
    extend_schema_group(
        &mut schemas,
        json!({
            "Hash": {
                "type": "string", "minLength": 64, "maxLength": 64,
                "pattern": "^[0-9A-Fa-f]{64}$",
                "examples": ["79cdcea38a54f99a59adddf124490a65de499e5677e693caa14dcf24b5f96007"]
            },
            "DateTime": {"type": "string", "format": "date-time"},
            "AtomicAmount": {
                "type": "string", "pattern": "^-?[0-9]+$",
                "description": "Exact signed amount in the chain's smallest atomic unit."
            },
            "Amount": {
                "type": "object", "additionalProperties": false,
                "required": ["zatoshi", "decimal", "symbol"],
                "properties": {
                    "zatoshi": {"$ref": "#/components/schemas/AtomicAmount"},
                    "decimal": {"type": "string", "pattern": "^-?[0-9]+\\.[0-9]{8}$"},
                    "symbol": {"type": "string"}
                }
            },
            "Meta": {
                "type": "object", "additionalProperties": false,
                "required": ["nextCursor", "indexedHeight", "nodeHeight", "freshnessSeconds", "network"],
                "properties": {
                    "nextCursor": nullable(json!({"type": "string"})),
                "indexedHeight": nullable(json!({"type": "integer", "format": "int64", "minimum": 0})),
                "nodeHeight": nullable(json!({"type": "integer", "format": "int64", "minimum": 0})),
                    "freshnessSeconds": nullable(json!({"type": "integer", "format": "int64", "minimum": 0})),
                    "network": {"type": "string"}
                }
            },
            "LiveResponse": {
                "type": "object", "additionalProperties": false,
                "required": ["status", "version"],
                "properties": {
                    "status": {"type": "string", "const": "live"},
                    "version": {"type": "string"}
                }
            },
            "ReadyResponse": {
                "type": "object", "additionalProperties": false,
                "required": ["status", "network", "indexedHeight", "nodeHeight", "updatedAt"],
                "properties": {
                    "status": {"type": "string", "const": "ready"},
                    "network": {"type": "string"},
                    "indexedHeight": {"type": "integer", "format": "int64", "minimum": 0},
                    "nodeHeight": {"type": "integer", "format": "int64", "minimum": 0},
                    "updatedAt": {"$ref": "#/components/schemas/DateTime"}
                }
            },
            "ProblemDetails": {
                "type": "object", "additionalProperties": false,
                "required": ["type", "title", "status", "detail", "instance"],
                "properties": {
                    "type": {"type": "string", "format": "uri"},
                    "title": {"type": "string"},
                    "status": {"type": "integer", "format": "int32", "minimum": 400, "maximum": 599},
                    "detail": {"type": "string"},
                    "instance": {"type": "string", "format": "uri"}
                }
            }
        }),
    );
    extend_schema_group(
        &mut schemas,
        json!({
            "Status": {
                "type": "object", "additionalProperties": false,
                "required": ["service", "version", "network", "networkName", "symbol", "status", "indexedHeight", "indexedHash", "nodeHeight", "nodeHash", "lagBlocks", "updatedAt", "uptimeSeconds", "difficulty", "observedSpacingSeconds", "targetSpacingSeconds", "totalIssued", "maxSupply", "initialSubsidy", "halvingInterval", "nextHalvingHeight", "coinbaseMaturity", "latestBlockTime", "privacyNotice"],
                "properties": {
                    "service": {"type": "string", "const": "wcashexplorer"},
                    "version": {"type": "string"},
                    "network": {"type": "string"},
                    "networkName": {"type": "string"},
                    "symbol": {"type": "string"},
                    "status": {"type": "string"},
                    "indexedHeight": nullable(json!({"type": "integer", "format": "int64", "minimum": 0})),
                    "indexedHash": nullable_ref("Hash"),
                    "nodeHeight": nullable(json!({"type": "integer", "format": "int64", "minimum": 0})),
                    "nodeHash": nullable_ref("Hash"),
                    "lagBlocks": nullable(json!({"type": "integer", "format": "int64", "minimum": 0})),
                    "updatedAt": {"$ref": "#/components/schemas/DateTime"},
                    "uptimeSeconds": {"type": "integer", "format": "int64", "minimum": 0},
                    "difficulty": nullable(json!({"type": "string"})),
                    "observedSpacingSeconds": nullable(json!({"type": "number", "format": "double", "minimum": 0})),
                    "targetSpacingSeconds": {"type": "integer", "format": "int32", "minimum": 1},
                    "totalIssued": {"$ref": "#/components/schemas/Amount"},
                    "maxSupply": {"$ref": "#/components/schemas/Amount"},
                    "initialSubsidy": {"$ref": "#/components/schemas/Amount"},
                    "halvingInterval": {"type": "integer", "format": "int64", "minimum": 1},
                    "nextHalvingHeight": {"type": "integer", "format": "int64", "minimum": 1},
                    "coinbaseMaturity": {"type": "integer", "format": "int32", "minimum": 0},
                    "latestBlockTime": nullable_ref("DateTime"),
                    "privacyNotice": {"type": "string"}
                }
            },
            "MergeMiningSummary": {
                "type": "object", "additionalProperties": false,
                "required": ["exactWitnessState", "localValidationState", "parentBlockHash", "parentLookupState", "parentSourcesAgree", "parentHeight", "parentConfirmations"],
                "properties": {
                    "exactWitnessState": {"type": "string"},
                    "localValidationState": {"type": "string"},
                    "parentBlockHash": nullable_ref("Hash"),
                    "parentLookupState": {"type": "string"},
                    "parentSourcesAgree": {"type": "boolean"},
                    "parentHeight": nullable(json!({"type": "integer", "format": "int64", "minimum": 0})),
                    "parentConfirmations": nullable(json!({"type": "integer", "format": "int64", "minimum": 0}))
                }
            },
            "BlockSummary": {
                "type": "object",
                "required": ["height", "hash", "witnessHash", "time", "sizeBytes", "transactionCount", "bits", "difficulty", "reward", "confirmations", "mergeMining"],
                "properties": {
                    "height": {"type": "integer", "format": "int64", "minimum": 0},
                    "hash": {"$ref": "#/components/schemas/Hash"},
                    "witnessHash": {"$ref": "#/components/schemas/Hash"},
                    "time": {"$ref": "#/components/schemas/DateTime"},
                    "sizeBytes": {"type": "integer", "format": "int64", "minimum": 0},
                    "transactionCount": {"type": "integer", "format": "int32", "minimum": 0},
                    "bits": {"type": "string"},
                    "difficulty": {"type": "string"},
                    "reward": {"$ref": "#/components/schemas/Amount"},
                    "confirmations": {"type": "integer", "format": "int64", "minimum": 0},
                    "mergeMining": {"$ref": "#/components/schemas/MergeMiningSummary"}
                }
            },
            "ValuePool": {
                "type": "object", "additionalProperties": false,
                "required": ["id", "chainValue", "valueDelta", "monitored"],
                "properties": {
                    "id": {"type": "string"},
                    "chainValue": nullable_ref("Amount"),
                    "valueDelta": nullable_ref("Amount"),
                    "monitored": nullable(json!({"type": "boolean"}))
                }
            }
        }),
    );
    extend_schema_group(
        &mut schemas,
        json!({
            "TransactionSummary": transaction_summary_schema(),
            "TransparentInput": {
                "type": "object", "additionalProperties": false,
                "required": ["inputIndex", "previousTxid", "previousOutputIndex", "coinbaseData", "sequence"],
                "properties": {
                    "inputIndex": {"type": "integer", "format": "int32", "minimum": 0},
                    "previousTxid": nullable_ref("Hash"),
                    "previousOutputIndex": nullable(json!({"type": "integer", "format": "int32", "minimum": 0})),
                    "coinbaseData": nullable(json!({"type": "string"})),
                    "sequence": nullable(json!({"type": "integer", "format": "int64", "minimum": 0}))
                }
            },
            "TransparentOutput": {
                "type": "object", "additionalProperties": false,
                "required": ["index", "value", "address", "scriptType", "scriptHex"],
                "properties": {
                    "index": {"type": "integer", "format": "int32", "minimum": 0},
                    "value": {"$ref": "#/components/schemas/Amount"},
                    "address": nullable(json!({"type": "string"})),
                    "scriptType": nullable(json!({"type": "string"})),
                    "scriptHex": nullable(json!({"type": "string", "pattern": "^(?:[0-9A-Fa-f]{2})*$"}))
                }
            },
            "ParentObservation": {
                "type": "object", "additionalProperties": false,
                "required": ["sourceName", "observationState", "parentHeight", "parentConfirmations", "parentTime", "parentBits", "parentDifficultyText", "embeddedHeaderMatches", "checkedAt"],
                "properties": {
                    "sourceName": {"type": "string"},
                    "observationState": {"type": "string"},
                    "parentHeight": nullable(json!({"type": "integer", "format": "int64", "minimum": 0})),
                    "parentConfirmations": nullable(json!({"type": "integer", "format": "int64"})),
                    "parentTime": nullable_ref("DateTime"),
                    "parentBits": nullable(json!({"type": "string"})),
                    "parentDifficultyText": nullable(json!({"type": "string"})),
                    "embeddedHeaderMatches": nullable(json!({"type": "boolean"})),
                    "checkedAt": {"$ref": "#/components/schemas/DateTime"}
                }
            },
            "AuxPow": {
                "type": "object", "additionalProperties": false,
                "required": ["proofVersion", "proofSize", "witnessHash", "exactWitnessState", "witnessConfirmations", "localValidationState", "verifierVersion", "parentBlockHash", "parentHeaderBits", "parentHashMeetsClaimedTarget", "parentCoinbaseTxid", "parentMerkleDepth", "parentCoinbaseIndex", "authDataMerkleDepth", "authDataCoinbaseIndex", "auxiliaryMerkleDepth", "auxiliaryIndex", "parentLookupState", "parentSourcesAgree", "verifiedAt", "parentBlockUrl", "parentCoinbaseTxUrl", "observations", "meaning"],
                "properties": {
                    "proofVersion": {"type": "integer", "format": "int32", "minimum": 0},
                    "proofSize": {"type": "integer", "format": "int64", "minimum": 0},
                    "witnessHash": {"$ref": "#/components/schemas/Hash"},
                    "exactWitnessState": {"type": "string"},
                    "witnessConfirmations": nullable(json!({"type": "integer", "format": "int64", "minimum": 0})),
                    "localValidationState": {"type": "string"},
                    "verifierVersion": nullable(json!({"type": "string"})),
                    "parentBlockHash": {"$ref": "#/components/schemas/Hash"},
                    "parentHeaderBits": {"type": "string"},
                    "parentHashMeetsClaimedTarget": {"type": "boolean"},
                    "parentCoinbaseTxid": {"$ref": "#/components/schemas/Hash"},
                    "parentMerkleDepth": {"type": "integer", "format": "int32", "minimum": 0},
                    "parentCoinbaseIndex": {"type": "integer", "format": "int64", "minimum": 0},
                    "authDataMerkleDepth": {"type": "integer", "format": "int32", "minimum": 0},
                    "authDataCoinbaseIndex": {"type": "integer", "format": "int64", "minimum": 0},
                    "auxiliaryMerkleDepth": {"type": "integer", "format": "int32", "minimum": 0},
                    "auxiliaryIndex": {"type": "integer", "format": "int64", "minimum": 0},
                    "parentLookupState": {"type": "string"},
                    "parentSourcesAgree": {"type": "boolean"},
                    "verifiedAt": {"$ref": "#/components/schemas/DateTime"},
                    "parentBlockUrl": {"type": "string", "format": "uri"},
                    "parentCoinbaseTxUrl": {"type": "string", "format": "uri"},
                    "observations": {"type": "array", "items": {"$ref": "#/components/schemas/ParentObservation"}},
                    "meaning": {"type": "string"}
                }
            }
        }),
    );
    extend_schema_group(
        &mut schemas,
        json!({
            "BlockDetail": {
                "unevaluatedProperties": false,
                "allOf": [
                    {"$ref": "#/components/schemas/BlockSummary"},
                    {
                        "type": "object",
                        "required": ["previousBlockHash", "nextBlockHash", "merkleRoot", "blockCommitments", "nonce", "valuePools", "transactions", "auxpow"],
                        "properties": {
                            "previousBlockHash": nullable_ref("Hash"),
                            "nextBlockHash": nullable_ref("Hash"),
                            "merkleRoot": {"$ref": "#/components/schemas/Hash"},
                            "blockCommitments": nullable_ref("Hash"),
                            "nonce": {"type": "string"},
                            "valuePools": {"type": "array", "items": {"$ref": "#/components/schemas/ValuePool"}},
                            "transactions": {"type": "array", "items": {"$ref": "#/components/schemas/TransactionSummary"}},
                            "auxpow": nullable_ref("AuxPow")
                        }
                    }
                ]
            },
            "RawBlock": {
                "type": "object", "additionalProperties": false,
                "required": ["blockHash", "witnessHash", "encoding", "data"],
                "properties": {
                    "blockHash": {"$ref": "#/components/schemas/Hash"},
                    "witnessHash": {"$ref": "#/components/schemas/Hash"},
                    "encoding": {"type": "string", "const": "hex"},
                    "data": {"type": "string", "pattern": "^(?:[0-9a-f]{2})*$"}
                }
            },
            "TransactionDetail": {
                "unevaluatedProperties": false,
                "allOf": [
                    {"$ref": "#/components/schemas/TransactionSummary"},
                    {
                        "type": "object",
                        "required": ["inputs", "outputs", "rawRpc", "privacyNotice"],
                        "properties": {
                            "inputs": {"type": "array", "items": {"$ref": "#/components/schemas/TransparentInput"}},
                            "outputs": {"type": "array", "items": {"$ref": "#/components/schemas/TransparentOutput"}},
                            "rawRpc": {"description": "Original indexed node RPC object. Its forward-compatible fields are not constrained by this API schema."},
                            "privacyNotice": {"type": "string"}
                        }
                    }
                ]
            },
            "AddressActivity": {
                "type": "object", "additionalProperties": false,
                "required": ["txid", "authDigest", "blockHeight", "blockHash", "blockTime", "received"],
                "properties": {
                    "txid": {"$ref": "#/components/schemas/Hash"},
                    "authDigest": {"$ref": "#/components/schemas/Hash"},
                    "blockHeight": {"type": "integer", "format": "int64", "minimum": 0},
                    "blockHash": {"$ref": "#/components/schemas/Hash"},
                    "blockTime": {"$ref": "#/components/schemas/DateTime"},
                    "received": {"$ref": "#/components/schemas/Amount"}
                }
            },
            "Address": {
                "type": "object", "additionalProperties": false,
                "required": ["address", "addressType", "totalReceived", "unspent", "immatureCoinbase", "matureCoinbaseMustShield", "utxoCount", "minedOutputCount", "coinbaseMaturity", "activity", "scopeNotice"],
                "properties": {
                    "address": {"type": "string"},
                    "addressType": {"type": "string", "const": "transparent"},
                    "totalReceived": {"$ref": "#/components/schemas/Amount"},
                    "unspent": {"$ref": "#/components/schemas/Amount"},
                    "immatureCoinbase": {"$ref": "#/components/schemas/Amount"},
                    "matureCoinbaseMustShield": {"$ref": "#/components/schemas/Amount"},
                    "utxoCount": {"type": "integer", "format": "int64", "minimum": 0},
                    "minedOutputCount": {"type": "integer", "format": "int64", "minimum": 0},
                    "coinbaseMaturity": {"type": "integer", "format": "int32", "minimum": 0},
                    "activity": {"type": "array", "items": {"$ref": "#/components/schemas/AddressActivity"}},
                    "scopeNotice": {"type": "string"}
                }
            },
            "Reorganization": {
                "type": "object", "additionalProperties": false,
                "required": ["eventId", "oldTipHeight", "oldTipHash", "commonAncestorHeight", "commonAncestorHash", "detectedAt", "completedAt"],
                "properties": {
                    "eventId": {"type": "string", "format": "uuid"},
                    "oldTipHeight": {"type": "integer", "format": "int64", "minimum": 0},
                    "oldTipHash": {"$ref": "#/components/schemas/Hash"},
                    "commonAncestorHeight": nullable(json!({"type": "integer", "format": "int64", "minimum": 0})),
                    "commonAncestorHash": nullable_ref("Hash"),
                    "detectedAt": {"$ref": "#/components/schemas/DateTime"},
                    "completedAt": nullable_ref("DateTime")
                }
            },
            "SearchResult": {
                "type": "object", "additionalProperties": false,
                "required": ["kind", "id", "route"],
                "properties": {
                    "kind": {"type": "string", "enum": ["block", "transaction", "address"]},
                    "id": {"type": "string"},
                    "route": {"type": "string", "pattern": "^/"}
                }
            },
            "StatusEnvelope": envelope_ref("Status"),
            "BlockListEnvelope": envelope_array_ref("BlockSummary"),
            "BlockDetailEnvelope": envelope_ref("BlockDetail"),
            "RawBlockEnvelope": envelope_ref("RawBlock"),
            "AuxPowEnvelope": envelope_schema(nullable_ref("AuxPow")),
            "TransactionListEnvelope": envelope_array_ref("TransactionSummary"),
            "TransactionDetailEnvelope": envelope_ref("TransactionDetail"),
            "AddressEnvelope": envelope_ref("Address"),
            "ReorgListEnvelope": envelope_array_ref("Reorganization"),
            "SearchEnvelope": envelope_ref("SearchResult"),
            "OpenApiDocument": {
                "type": "object",
                "required": ["openapi", "info", "paths"],
                "properties": {
                    "openapi": {"type": "string", "const": "3.1.0"},
                    "info": {"type": "object"},
                    "paths": {"type": "object"}
                },
                "additionalProperties": true
            }
        }),
    );
    Value::Object(schemas)
}

fn extend_schema_group(target: &mut serde_json::Map<String, Value>, group: Value) {
    let Value::Object(group) = group else {
        unreachable!("OpenAPI schema groups are JSON objects")
    };
    target.extend(group);
}

fn transaction_summary_schema() -> Value {
    json!({
        "type": "object",
        "required": ["txid", "authDigest", "version", "sizeBytes", "isCoinbase", "kind", "fee", "publicOutputValue", "valueBalance", "transparentInputCount", "transparentOutputCount", "saplingSpendCount", "saplingOutputCount", "orchardActionCount", "ironwoodActionCount", "blockHeight", "blockHash", "blockTime", "position"],
        "properties": {
            "txid": {"$ref": "#/components/schemas/Hash"},
            "authDigest": {"$ref": "#/components/schemas/Hash"},
            "version": {"type": "integer", "format": "int64"},
            "sizeBytes": {"type": "integer", "format": "int64", "minimum": 0},
            "isCoinbase": {"type": "boolean"},
            "kind": {"type": "string", "enum": ["coinbase", "transparent", "shielded", "mixed"]},
            "fee": nullable_ref("Amount"),
            "publicOutputValue": {"$ref": "#/components/schemas/Amount"},
            "valueBalance": nullable_ref("Amount"),
            "transparentInputCount": {"type": "integer", "format": "int64", "minimum": 0},
            "transparentOutputCount": {"type": "integer", "format": "int64", "minimum": 0},
            "saplingSpendCount": {"type": "integer", "format": "int32", "minimum": 0},
            "saplingOutputCount": {"type": "integer", "format": "int32", "minimum": 0},
            "orchardActionCount": {"type": "integer", "format": "int32", "minimum": 0},
            "ironwoodActionCount": {"type": "integer", "format": "int32", "minimum": 0},
            "blockHeight": {"type": "integer", "format": "int64", "minimum": 0},
            "blockHash": {"$ref": "#/components/schemas/Hash"},
            "blockTime": {"$ref": "#/components/schemas/DateTime"},
            "position": {"type": "integer", "format": "int32", "minimum": 0}
        }
    })
}

fn nullable(schema: impl Into<Value>) -> Value {
    let schema = schema.into();
    json!({"oneOf": [schema, {"type": "null"}]})
}

fn nullable_ref(name: &str) -> Value {
    nullable(schema_ref(name))
}

fn envelope_schema(data_schema: impl Into<Value>) -> Value {
    let data_schema = data_schema.into();
    json!({
        "type": "object", "additionalProperties": false,
        "required": ["data", "meta"],
        "properties": {
            "data": data_schema,
            "meta": {"$ref": "#/components/schemas/Meta"}
        }
    })
}

fn envelope_ref(name: &str) -> Value {
    envelope_schema(schema_ref(name))
}

fn envelope_array_ref(name: &str) -> Value {
    envelope_schema(json!({"type": "array", "items": schema_ref(name)}))
}

async fn find_block(state: &AppState, id: &str) -> Result<BlockDetailRow> {
    let height = id.parse::<u64>().ok();
    let hash = if height.is_none() {
        ensure_hash("block hash", id)?;
        Some(id)
    } else {
        None
    };
    sqlx::query_as::<_, BlockDetailRow>(BLOCK_DETAIL_QUERY)
        .bind(&state.network.id)
        .bind(
            height
                .map(|value| to_i64(value, "block height"))
                .transpose()?,
        )
        .bind(hash)
        .fetch_optional(state.database.pool())
        .await?
        .ok_or(ExplorerError::NotFound)
}

async fn load_auxpow(
    state: &AppState,
    block_hash: &str,
    witness_hash: &str,
) -> Result<Option<AuxPowView>> {
    let row = sqlx::query_as::<_, AuxPowRow>(AUXPOW_QUERY)
        .bind(block_hash)
        .bind(witness_hash)
        .fetch_optional(state.database.pool())
        .await?;
    let Some(row) = row else { return Ok(None) };
    let observations = sqlx::query_as::<_, ParentObservationView>(
        "SELECT source_name, observation_state, parent_height,
                parent_confirmations, parent_time, parent_bits,
                parent_difficulty_text, embedded_header_matches, checked_at
         FROM parent_chain_observations
         WHERE block_hash = $1 AND witness_hash = $2
         ORDER BY source_name",
    )
    .bind(block_hash)
    .bind(witness_hash)
    .fetch_all(state.database.pool())
    .await?;
    let parent_block_url = state
        .network
        .parent_explorer_block_url
        .replace("{hash}", &row.parent_block_hash);
    let parent_coinbase_tx_url = state
        .network
        .parent_explorer_tx_url
        .replace("{txid}", &row.parent_coinbase_txid);
    Ok(Some(AuxPowView {
        proof_version: row.proof_version,
        proof_size: row.proof_size,
        witness_hash: witness_hash.to_owned(),
        exact_witness_state: row.exact_witness_state,
        witness_confirmations: row.witness_confirmations,
        local_validation_state: row.local_validation_state,
        verifier_version: row.verifier_version,
        parent_block_hash: row.parent_block_hash,
        parent_header_bits: row.parent_header_bits,
        parent_hash_meets_claimed_target: row.parent_hash_meets_claimed_target,
        parent_coinbase_txid: row.parent_coinbase_txid,
        parent_merkle_depth: row.parent_merkle_depth,
        parent_coinbase_index: row.parent_coinbase_index,
        auth_data_merkle_depth: row.auth_data_merkle_depth,
        auth_data_coinbase_index: row.auth_data_coinbase_index,
        auxiliary_merkle_depth: row.auxiliary_merkle_depth,
        auxiliary_index: row.auxiliary_index,
        parent_lookup_state: row.parent_lookup_state,
        parent_sources_agree: row.parent_sources_agree,
        verified_at: row.verified_at,
        parent_block_url,
        parent_coinbase_tx_url,
        observations,
        meaning: "AuxPoW validity is verified locally. Parent status is reported by configured Zcash nodes.".to_owned(),
    }))
}

async fn transaction_rows_for_block(
    state: &AppState,
    block_hash: &str,
    witness_hash: &str,
) -> Result<Vec<TransactionSummary>> {
    sqlx::query_as::<_, TransactionSummaryRow>(TRANSACTIONS_FOR_BLOCK_QUERY)
        .bind(block_hash)
        .bind(witness_hash)
        .fetch_all(state.database.pool())
        .await?
        .into_iter()
        .map(|row| TransactionSummary::from_row(row, &state.network))
        .collect()
}

async fn chain_state(state: &AppState) -> Result<ChainStateRow> {
    sqlx::query_as::<_, ChainStateRow>(
        "SELECT indexed_height, indexed_hash, node_height, node_hash, status, updated_at
         FROM chain_state WHERE network_id = $1",
    )
    .bind(&state.network.id)
    .fetch_one(state.database.pool())
    .await
    .map_err(Into::into)
}

fn envelope<T: Serialize>(
    state: &AppState,
    data: T,
    next_cursor: Option<String>,
    chain: &ChainStateRow,
) -> ApiEnvelope<T> {
    ApiEnvelope {
        data,
        meta: PageMeta {
            next_cursor,
            indexed_height: chain
                .indexed_height
                .and_then(|value| u64::try_from(value).ok()),
            node_height: chain
                .node_height
                .and_then(|value| u64::try_from(value).ok()),
            freshness_seconds: Some((Utc::now() - chain.updated_at).num_seconds().max(0)),
            network: state.network.id.clone(),
        },
    }
}

fn amount(zatoshi: i64, network: &NetworkConfig) -> AmountView {
    let divisor = 10_i128.pow(u32::from(network.decimals));
    let value = i128::from(zatoshi);
    let sign = if value < 0 { "-" } else { "" };
    let absolute = value.abs();
    let whole = absolute / divisor;
    let fractional = absolute % divisor;
    AmountView {
        zatoshi: zatoshi.to_string(),
        decimal: format!(
            "{sign}{whole}.{fractional:0width$}",
            width = usize::from(network.decimals)
        ),
        symbol: network.symbol.clone(),
    }
}

fn next_halving_height(height: u64, network: &NetworkConfig) -> u64 {
    if height < network.first_halving_height {
        return network.first_halving_height;
    }
    let elapsed = height - network.first_halving_height;
    network.first_halving_height
        + (elapsed / network.halving_interval + 1) * network.halving_interval
}

fn bounded_limit(limit: Option<u16>) -> u16 {
    limit.unwrap_or(DEFAULT_PAGE_SIZE).clamp(1, MAX_PAGE_SIZE)
}

fn encode_cursor(height: u64) -> String {
    URL_SAFE_NO_PAD.encode(height.to_be_bytes())
}

fn decode_cursor(value: &str) -> Result<i64> {
    let bytes = URL_SAFE_NO_PAD
        .decode(value)
        .map_err(|_| ExplorerError::InvalidRequest("invalid pagination cursor".to_owned()))?;
    let bytes: [u8; 8] = bytes
        .try_into()
        .map_err(|_| ExplorerError::InvalidRequest("invalid pagination cursor".to_owned()))?;
    to_i64(u64::from_be_bytes(bytes), "cursor")
}

fn encode_tx_cursor(height: u64, position: i32) -> String {
    let mut bytes = Vec::with_capacity(12);
    bytes.extend_from_slice(&height.to_be_bytes());
    bytes.extend_from_slice(&position.to_be_bytes());
    URL_SAFE_NO_PAD.encode(bytes)
}

fn decode_tx_cursor(value: &str) -> Result<(u64, i32)> {
    let bytes = URL_SAFE_NO_PAD
        .decode(value)
        .map_err(|_| ExplorerError::InvalidRequest("invalid pagination cursor".to_owned()))?;
    if bytes.len() != 12 {
        return Err(ExplorerError::InvalidRequest(
            "invalid pagination cursor".to_owned(),
        ));
    }
    let height = u64::from_be_bytes(bytes[..8].try_into().expect("length checked"));
    let position = i32::from_be_bytes(bytes[8..].try_into().expect("length checked"));
    if position < 0 {
        return Err(ExplorerError::InvalidRequest(
            "invalid pagination cursor".to_owned(),
        ));
    }
    Ok((height, position))
}

fn ensure_hash(label: &str, value: &str) -> Result<()> {
    if !is_hash(value) {
        return Err(ExplorerError::InvalidRequest(format!(
            "{label} must be exactly 32 bytes of hexadecimal"
        )));
    }
    Ok(())
}

fn is_hash(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn validate_address(value: &str) -> Result<()> {
    if !(20..=256).contains(&value.len())
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
    {
        return Err(ExplorerError::InvalidRequest(
            "address has an invalid public encoding".to_owned(),
        ));
    }
    Ok(())
}

fn to_u64(value: i64, label: &str) -> Result<u64> {
    u64::try_from(value)
        .map_err(|_| ExplorerError::InvalidNodeResponse(format!("negative {label} in index")))
}

fn optional_u64(value: Option<i64>, label: &str) -> Result<Option<u64>> {
    value.map(|value| to_u64(value, label)).transpose()
}

fn to_i64(value: u64, label: &str) -> Result<i64> {
    i64::try_from(value)
        .map_err(|_| ExplorerError::InvalidRequest(format!("{label} exceeds the supported range")))
}

#[derive(Debug, Deserialize)]
struct PageQuery {
    cursor: Option<String>,
    limit: Option<u16>,
}

#[derive(Debug, Deserialize)]
struct TransactionQuery {
    block: Option<String>,
}

#[derive(Debug, Deserialize)]
struct SearchQuery {
    q: String,
}

#[derive(Clone, Debug, FromRow)]
struct ChainStateRow {
    indexed_height: Option<i64>,
    indexed_hash: Option<String>,
    node_height: Option<i64>,
    node_hash: Option<String>,
    status: String,
    updated_at: DateTime<Utc>,
}

#[derive(Clone, Debug, FromRow)]
struct LatestChainRow {
    difficulty_text: String,
    chain_supply_zat: Option<i64>,
    block_time: DateTime<Utc>,
    observed_spacing_seconds: Option<f64>,
}

#[derive(Clone, Debug, FromRow)]
struct BlockSummaryRow {
    height: i64,
    hash: String,
    witness_hash: String,
    block_time: DateTime<Utc>,
    size_bytes: i64,
    transaction_count: i32,
    bits: String,
    difficulty_text: String,
    reward_zat: i64,
    exact_witness_state: String,
    witness_confirmations: Option<i64>,
    local_validation_state: String,
    parent_block_hash: Option<String>,
    parent_lookup_state: Option<String>,
    parent_sources_agree: Option<bool>,
    parent_height: Option<i64>,
    parent_confirmations: Option<i64>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BlockSummary {
    pub height: u64,
    pub hash: String,
    pub witness_hash: String,
    pub time: DateTime<Utc>,
    pub size_bytes: i64,
    pub transaction_count: i32,
    pub bits: String,
    pub difficulty: String,
    pub reward: AmountView,
    pub confirmations: u64,
    pub merge_mining: MergeMiningSummary,
}

impl BlockSummary {
    fn from_row(row: BlockSummaryRow, network: &NetworkConfig) -> Result<Self> {
        Ok(Self {
            height: to_u64(row.height, "block height")?,
            hash: row.hash,
            witness_hash: row.witness_hash,
            time: row.block_time,
            size_bytes: row.size_bytes,
            transaction_count: row.transaction_count,
            bits: row.bits,
            difficulty: row.difficulty_text,
            reward: amount(row.reward_zat, network),
            confirmations: u64::try_from(row.witness_confirmations.unwrap_or(0)).unwrap_or(0),
            merge_mining: MergeMiningSummary {
                exact_witness_state: row.exact_witness_state,
                local_validation_state: row.local_validation_state,
                parent_block_hash: row.parent_block_hash,
                parent_lookup_state: row
                    .parent_lookup_state
                    .unwrap_or_else(|| "genesis".to_owned()),
                parent_sources_agree: row.parent_sources_agree.unwrap_or(false),
                parent_height: row
                    .parent_height
                    .and_then(|height| u64::try_from(height).ok()),
                parent_confirmations: row
                    .parent_confirmations
                    .and_then(|confirmations| u64::try_from(confirmations).ok()),
            },
        })
    }
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MergeMiningSummary {
    pub exact_witness_state: String,
    pub local_validation_state: String,
    pub parent_block_hash: Option<String>,
    pub parent_lookup_state: String,
    pub parent_sources_agree: bool,
    pub parent_height: Option<u64>,
    pub parent_confirmations: Option<u64>,
}

#[derive(Clone, Debug, FromRow)]
struct BlockDetailRow {
    #[sqlx(flatten)]
    summary: BlockSummaryRow,
    previous_block_hash: Option<String>,
    next_block_hash: Option<String>,
    merkle_root: String,
    block_commitments: Option<String>,
    nonce: String,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BlockDetail {
    #[serde(flatten)]
    pub summary: BlockSummary,
    pub previous_block_hash: Option<String>,
    pub next_block_hash: Option<String>,
    pub merkle_root: String,
    pub block_commitments: Option<String>,
    pub nonce: String,
    pub value_pools: Vec<ValuePoolView>,
    pub transactions: Vec<TransactionSummary>,
    pub auxpow: Option<AuxPowView>,
}

#[derive(Clone, Debug, FromRow)]
struct ValuePoolRow {
    pool_id: String,
    chain_value_zat: Option<i64>,
    value_delta_zat: Option<i64>,
    monitored: Option<bool>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ValuePoolView {
    pub id: String,
    pub chain_value: Option<AmountView>,
    pub value_delta: Option<AmountView>,
    pub monitored: Option<bool>,
}

#[derive(Clone, Debug, FromRow)]
struct TransactionSummaryRow {
    txid: String,
    auth_digest: String,
    version: i64,
    size_bytes: i64,
    is_coinbase: bool,
    fee_zat: Option<i64>,
    value_balance_zat: Option<i64>,
    sapling_spend_count: i32,
    sapling_output_count: i32,
    orchard_action_count: i32,
    ironwood_action_count: i32,
    block_height: i64,
    block_hash: String,
    block_time: DateTime<Utc>,
    tx_position: i32,
    transparent_input_count: i64,
    transparent_output_count: i64,
    public_output_zat: i64,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TransactionSummary {
    pub txid: String,
    pub auth_digest: String,
    pub version: i64,
    pub size_bytes: i64,
    pub is_coinbase: bool,
    pub kind: String,
    pub fee: Option<AmountView>,
    pub public_output_value: AmountView,
    pub value_balance: Option<AmountView>,
    pub transparent_input_count: i64,
    pub transparent_output_count: i64,
    pub sapling_spend_count: i32,
    pub sapling_output_count: i32,
    pub orchard_action_count: i32,
    pub ironwood_action_count: i32,
    pub block_height: u64,
    pub block_hash: String,
    pub block_time: DateTime<Utc>,
    pub position: i32,
}

impl TransactionSummary {
    fn from_row(row: TransactionSummaryRow, network: &NetworkConfig) -> Result<Self> {
        let has_transparent = row.transparent_input_count > 0 || row.transparent_output_count > 0;
        let has_shielded = row.sapling_spend_count > 0
            || row.sapling_output_count > 0
            || row.orchard_action_count > 0
            || row.ironwood_action_count > 0;
        let kind = if row.is_coinbase {
            "coinbase"
        } else if has_transparent && has_shielded {
            "mixed"
        } else if has_shielded {
            "shielded"
        } else {
            "transparent"
        };
        Ok(Self {
            txid: row.txid,
            auth_digest: row.auth_digest,
            version: row.version,
            size_bytes: row.size_bytes,
            is_coinbase: row.is_coinbase,
            kind: kind.to_owned(),
            fee: row.fee_zat.map(|value| amount(value, network)),
            public_output_value: amount(row.public_output_zat, network),
            value_balance: row.value_balance_zat.map(|value| amount(value, network)),
            transparent_input_count: row.transparent_input_count,
            transparent_output_count: row.transparent_output_count,
            sapling_spend_count: row.sapling_spend_count,
            sapling_output_count: row.sapling_output_count,
            orchard_action_count: row.orchard_action_count,
            ironwood_action_count: row.ironwood_action_count,
            block_height: to_u64(row.block_height, "block height")?,
            block_hash: row.block_hash,
            block_time: row.block_time,
            position: row.tx_position,
        })
    }
}

#[derive(Clone, Debug, Serialize, FromRow)]
#[serde(rename_all = "camelCase")]
pub struct InputView {
    pub input_index: i32,
    pub previous_txid: Option<String>,
    pub previous_output_index: Option<i32>,
    pub coinbase_data: Option<String>,
    pub sequence: Option<i64>,
}

#[derive(Clone, Debug, FromRow)]
struct OutputRow {
    output_index: i32,
    value_zat: i64,
    address: Option<String>,
    script_type: Option<String>,
    script_hex: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OutputView {
    pub index: i32,
    pub value: AmountView,
    pub address: Option<String>,
    pub script_type: Option<String>,
    pub script_hex: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TransactionDetail {
    #[serde(flatten)]
    pub summary: TransactionSummary,
    pub inputs: Vec<InputView>,
    pub outputs: Vec<OutputView>,
    pub raw_rpc: Value,
    pub privacy_notice: String,
}

#[derive(Clone, Debug, FromRow)]
struct AuxPowRow {
    proof_version: i16,
    proof_size: i64,
    parent_block_hash: String,
    parent_header_bits: String,
    parent_hash_meets_claimed_target: bool,
    parent_coinbase_txid: String,
    parent_merkle_depth: i32,
    parent_coinbase_index: i64,
    auth_data_merkle_depth: i32,
    auth_data_coinbase_index: i64,
    auxiliary_merkle_depth: i32,
    auxiliary_index: i64,
    parent_lookup_state: String,
    parent_sources_agree: bool,
    verified_at: DateTime<Utc>,
    exact_witness_state: String,
    witness_confirmations: Option<i64>,
    local_validation_state: String,
    verifier_version: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AuxPowView {
    pub proof_version: i16,
    pub proof_size: i64,
    pub witness_hash: String,
    pub exact_witness_state: String,
    pub witness_confirmations: Option<i64>,
    pub local_validation_state: String,
    pub verifier_version: Option<String>,
    pub parent_block_hash: String,
    pub parent_header_bits: String,
    pub parent_hash_meets_claimed_target: bool,
    pub parent_coinbase_txid: String,
    pub parent_merkle_depth: i32,
    pub parent_coinbase_index: i64,
    pub auth_data_merkle_depth: i32,
    pub auth_data_coinbase_index: i64,
    pub auxiliary_merkle_depth: i32,
    pub auxiliary_index: i64,
    pub parent_lookup_state: String,
    pub parent_sources_agree: bool,
    pub verified_at: DateTime<Utc>,
    pub parent_block_url: String,
    pub parent_coinbase_tx_url: String,
    pub observations: Vec<ParentObservationView>,
    pub meaning: String,
}

#[derive(Clone, Debug, Serialize, FromRow)]
#[serde(rename_all = "camelCase")]
pub struct ParentObservationView {
    pub source_name: String,
    pub observation_state: String,
    pub parent_height: Option<i64>,
    pub parent_confirmations: Option<i64>,
    pub parent_time: Option<DateTime<Utc>>,
    pub parent_bits: Option<String>,
    pub parent_difficulty_text: Option<String>,
    pub embedded_header_matches: Option<bool>,
    pub checked_at: DateTime<Utc>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AmountView {
    pub zatoshi: String,
    pub decimal: String,
    pub symbol: String,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StatusView {
    pub service: String,
    pub version: String,
    pub network: String,
    pub network_name: String,
    pub symbol: String,
    pub status: String,
    pub indexed_height: Option<u64>,
    pub indexed_hash: Option<String>,
    pub node_height: Option<u64>,
    pub node_hash: Option<String>,
    pub lag_blocks: Option<i64>,
    pub updated_at: DateTime<Utc>,
    pub uptime_seconds: i64,
    pub difficulty: Option<String>,
    pub observed_spacing_seconds: Option<f64>,
    pub target_spacing_seconds: u32,
    pub total_issued: AmountView,
    pub max_supply: AmountView,
    pub initial_subsidy: AmountView,
    pub halving_interval: u64,
    pub next_halving_height: u64,
    pub coinbase_maturity: u32,
    pub latest_block_time: Option<DateTime<Utc>>,
    pub privacy_notice: String,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RawBlockView {
    pub block_hash: String,
    pub witness_hash: String,
    pub encoding: String,
    pub data: String,
}

#[derive(Clone, Debug, FromRow)]
struct AddressBalanceRow {
    total_received_zat: i64,
    unspent_zat: i64,
    immature_coinbase_zat: i64,
    mature_coinbase_zat: i64,
    utxo_count: i64,
    mined_output_count: i64,
}

#[derive(Clone, Debug, FromRow)]
struct AddressActivityRow {
    block_height: i64,
    block_hash: String,
    block_time: DateTime<Utc>,
    txid: String,
    auth_digest: String,
    received_zat: i64,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AddressView {
    pub address: String,
    pub address_type: String,
    pub total_received: AmountView,
    pub unspent: AmountView,
    pub immature_coinbase: AmountView,
    pub mature_coinbase_must_shield: AmountView,
    pub utxo_count: i64,
    pub mined_output_count: i64,
    pub coinbase_maturity: u32,
    pub activity: Vec<AddressActivity>,
    pub scope_notice: String,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AddressActivity {
    pub txid: String,
    pub auth_digest: String,
    pub block_height: u64,
    pub block_hash: String,
    pub block_time: DateTime<Utc>,
    pub received: AmountView,
}

#[derive(Clone, Debug, Serialize, FromRow)]
#[serde(rename_all = "camelCase")]
pub struct ReorgView {
    pub event_id: Uuid,
    pub old_tip_height: i64,
    pub old_tip_hash: String,
    pub common_ancestor_height: Option<i64>,
    pub common_ancestor_hash: Option<String>,
    pub detected_at: DateTime<Utc>,
    pub completed_at: Option<DateTime<Utc>>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchResult {
    pub kind: String,
    pub id: String,
    pub route: String,
}

impl SearchResult {
    fn new(kind: &str, id: &str, route: String) -> Self {
        Self {
            kind: kind.to_owned(),
            id: id.to_owned(),
            route,
        }
    }
}

const BLOCK_SUMMARY_QUERY: &str =
    "SELECT c.height, c.block_hash AS hash, c.witness_hash, b.block_time,
            b.size_bytes, b.transaction_count, b.bits, b.difficulty_text,
            COALESCE(
                b.chain_supply_zat - COALESCE((
                    SELECT previous_block.chain_supply_zat
                    FROM canonical_chain previous_chain
                    JOIN blocks previous_block ON previous_block.block_hash = previous_chain.block_hash
                    WHERE previous_chain.network_id = c.network_id
                      AND previous_chain.height = c.height - 1
                ), 0),
                (
                    SELECT SUM(o.value_zat)
                    FROM block_transactions reward_bt
                    JOIN transaction_instances reward_tx ON reward_tx.transaction_instance_id = reward_bt.transaction_instance_id
                    JOIN transparent_outputs o ON o.transaction_instance_id = reward_tx.transaction_instance_id
                    WHERE reward_bt.block_hash = c.block_hash
                      AND reward_bt.witness_hash = c.witness_hash
                      AND reward_tx.is_coinbase
                ),
                0
            )::BIGINT AS reward_zat,
            w.exact_witness_state,
            ((SELECT MAX(tip.height) FROM canonical_chain tip WHERE tip.network_id = c.network_id) - c.height + 1)::BIGINT AS witness_confirmations,
            w.local_validation_state, a.parent_block_hash,
            a.parent_lookup_state, a.parent_sources_agree,
            (SELECT MAX(p.parent_height) FROM parent_chain_observations p
             WHERE p.block_hash = c.block_hash AND p.witness_hash = c.witness_hash
               AND p.observation_state = 'canonical') AS parent_height,
            (SELECT MAX(p.parent_confirmations) FROM parent_chain_observations p
             WHERE p.block_hash = c.block_hash AND p.witness_hash = c.witness_hash
               AND p.observation_state = 'canonical') AS parent_confirmations
     FROM canonical_chain c
     JOIN blocks b ON b.block_hash = c.block_hash
     JOIN block_witnesses w ON w.block_hash = c.block_hash AND w.witness_hash = c.witness_hash
     LEFT JOIN auxpow_links a ON a.block_hash = c.block_hash AND a.witness_hash = c.witness_hash
     WHERE c.network_id = $1 AND ($2::BIGINT IS NULL OR c.height < $2)
     ORDER BY c.height DESC LIMIT $3";

const BLOCK_DETAIL_QUERY: &str =
    "SELECT c.height, c.block_hash AS hash, c.witness_hash, b.block_time,
            b.size_bytes, b.transaction_count, b.bits, b.difficulty_text,
            COALESCE(
                b.chain_supply_zat - COALESCE((
                    SELECT previous_block.chain_supply_zat
                    FROM canonical_chain previous_chain
                    JOIN blocks previous_block ON previous_block.block_hash = previous_chain.block_hash
                    WHERE previous_chain.network_id = c.network_id
                      AND previous_chain.height = c.height - 1
                ), 0),
                (
                    SELECT SUM(o.value_zat)
                    FROM block_transactions reward_bt
                    JOIN transaction_instances reward_tx ON reward_tx.transaction_instance_id = reward_bt.transaction_instance_id
                    JOIN transparent_outputs o ON o.transaction_instance_id = reward_tx.transaction_instance_id
                    WHERE reward_bt.block_hash = c.block_hash
                      AND reward_bt.witness_hash = c.witness_hash
                      AND reward_tx.is_coinbase
                ),
                0
            )::BIGINT AS reward_zat,
            w.exact_witness_state,
            ((SELECT MAX(tip.height) FROM canonical_chain tip WHERE tip.network_id = c.network_id) - c.height + 1)::BIGINT AS witness_confirmations,
            w.local_validation_state, a.parent_block_hash,
            a.parent_lookup_state, a.parent_sources_agree,
            (SELECT MAX(p.parent_height) FROM parent_chain_observations p
             WHERE p.block_hash = c.block_hash AND p.witness_hash = c.witness_hash
               AND p.observation_state = 'canonical') AS parent_height,
            (SELECT MAX(p.parent_confirmations) FROM parent_chain_observations p
             WHERE p.block_hash = c.block_hash AND p.witness_hash = c.witness_hash
               AND p.observation_state = 'canonical') AS parent_confirmations,
            b.previous_block_hash,
            (SELECT next_c.block_hash FROM canonical_chain next_c
             WHERE next_c.network_id = c.network_id AND next_c.height = c.height + 1) AS next_block_hash,
            b.merkle_root, b.block_commitments, b.nonce
     FROM canonical_chain c
     JOIN blocks b ON b.block_hash = c.block_hash
     JOIN block_witnesses w ON w.block_hash = c.block_hash AND w.witness_hash = c.witness_hash
     LEFT JOIN auxpow_links a ON a.block_hash = c.block_hash AND a.witness_hash = c.witness_hash
     WHERE c.network_id = $1
       AND (($2::BIGINT IS NOT NULL AND c.height = $2) OR ($3::TEXT IS NOT NULL AND c.block_hash = $3))
     LIMIT 1";

const TRANSACTION_SUMMARY_QUERY: &str =
    "SELECT ti.txid, ti.auth_digest, ti.version, ti.size_bytes, ti.is_coinbase,
            ti.fee_zat, ti.value_balance_zat, ti.sapling_spend_count,
            ti.sapling_output_count, ti.orchard_action_count, ti.ironwood_action_count,
            c.height AS block_height, c.block_hash, b.block_time, bt.tx_position,
            (SELECT COUNT(*) FROM transparent_inputs i WHERE i.transaction_instance_id = ti.transaction_instance_id) AS transparent_input_count,
            (SELECT COUNT(*) FROM transparent_outputs o WHERE o.transaction_instance_id = ti.transaction_instance_id) AS transparent_output_count,
            (SELECT COALESCE(SUM(o.value_zat), 0)::BIGINT FROM transparent_outputs o WHERE o.transaction_instance_id = ti.transaction_instance_id) AS public_output_zat
     FROM canonical_chain c
     JOIN blocks b ON b.block_hash = c.block_hash
     JOIN block_transactions bt ON bt.block_hash = c.block_hash AND bt.witness_hash = c.witness_hash
     JOIN transaction_instances ti ON ti.transaction_instance_id = bt.transaction_instance_id
     WHERE c.network_id = $1
       AND ($2::BIGINT IS NULL OR c.height < $2 OR (c.height = $2 AND bt.tx_position < $3))
     ORDER BY c.height DESC, bt.tx_position DESC
     LIMIT $4";

const TRANSACTION_DETAIL_QUERY: &str =
    "SELECT ti.txid, ti.auth_digest, ti.version, ti.size_bytes, ti.is_coinbase,
            ti.fee_zat, ti.value_balance_zat, ti.sapling_spend_count,
            ti.sapling_output_count, ti.orchard_action_count, ti.ironwood_action_count,
            c.height AS block_height, c.block_hash, b.block_time, bt.tx_position,
            (SELECT COUNT(*) FROM transparent_inputs i WHERE i.transaction_instance_id = ti.transaction_instance_id) AS transparent_input_count,
            (SELECT COUNT(*) FROM transparent_outputs o WHERE o.transaction_instance_id = ti.transaction_instance_id) AS transparent_output_count,
            (SELECT COALESCE(SUM(o.value_zat), 0)::BIGINT FROM transparent_outputs o WHERE o.transaction_instance_id = ti.transaction_instance_id) AS public_output_zat
     FROM transaction_instances ti
     JOIN block_transactions bt ON bt.transaction_instance_id = ti.transaction_instance_id
     JOIN canonical_chain c ON c.block_hash = bt.block_hash AND c.witness_hash = bt.witness_hash
     JOIN blocks b ON b.block_hash = c.block_hash
     WHERE c.network_id = $1 AND ti.txid = $2
       AND ($3::TEXT IS NULL OR c.block_hash = $3)
     ORDER BY c.height DESC LIMIT 1";

const TRANSACTIONS_FOR_BLOCK_QUERY: &str =
    "SELECT ti.txid, ti.auth_digest, ti.version, ti.size_bytes, ti.is_coinbase,
            ti.fee_zat, ti.value_balance_zat, ti.sapling_spend_count,
            ti.sapling_output_count, ti.orchard_action_count, ti.ironwood_action_count,
            b.height AS block_height, b.block_hash, b.block_time, bt.tx_position,
            (SELECT COUNT(*) FROM transparent_inputs i WHERE i.transaction_instance_id = ti.transaction_instance_id) AS transparent_input_count,
            (SELECT COUNT(*) FROM transparent_outputs o WHERE o.transaction_instance_id = ti.transaction_instance_id) AS transparent_output_count,
            (SELECT COALESCE(SUM(o.value_zat), 0)::BIGINT FROM transparent_outputs o WHERE o.transaction_instance_id = ti.transaction_instance_id) AS public_output_zat
     FROM block_transactions bt
     JOIN transaction_instances ti ON ti.transaction_instance_id = bt.transaction_instance_id
     JOIN blocks b ON b.block_hash = bt.block_hash
     WHERE bt.block_hash = $1 AND bt.witness_hash = $2
     ORDER BY bt.tx_position";

const AUXPOW_QUERY: &str = "SELECT a.proof_version, a.proof_size, a.parent_block_hash,
            a.parent_header_bits, a.parent_hash_meets_claimed_target,
            a.parent_coinbase_txid, a.parent_merkle_depth,
            a.parent_coinbase_index, a.auth_data_merkle_depth,
            a.auth_data_coinbase_index, a.auxiliary_merkle_depth,
            a.auxiliary_index, a.parent_lookup_state,
            a.parent_sources_agree, a.verified_at,
            w.exact_witness_state, w.witness_confirmations,
            w.local_validation_state, w.verifier_version
     FROM auxpow_links a
     JOIN block_witnesses w ON w.block_hash = a.block_hash AND w.witness_hash = a.witness_hash
     WHERE a.block_hash = $1 AND a.witness_hash = $2";

const ADDRESS_BALANCE_QUERY: &str =
    "WITH owned AS (
        SELECT o.value_zat, ti.txid, o.output_index, ti.is_coinbase, c.height,
               NOT EXISTS (
                   SELECT 1
                   FROM transparent_inputs spending_input
                   JOIN transaction_instances spending_tx ON spending_tx.transaction_instance_id = spending_input.transaction_instance_id
                   JOIN block_transactions spending_bt ON spending_bt.transaction_instance_id = spending_tx.transaction_instance_id
                   JOIN canonical_chain spending_c ON spending_c.block_hash = spending_bt.block_hash AND spending_c.witness_hash = spending_bt.witness_hash
                   WHERE spending_c.network_id = $1
                     AND spending_input.previous_txid = ti.txid
                     AND spending_input.previous_output_index = o.output_index
               ) AS unspent
        FROM transparent_outputs o
        JOIN transaction_instances ti ON ti.transaction_instance_id = o.transaction_instance_id
        JOIN block_transactions bt ON bt.transaction_instance_id = ti.transaction_instance_id
        JOIN canonical_chain c ON c.block_hash = bt.block_hash AND c.witness_hash = bt.witness_hash
        WHERE c.network_id = $1 AND o.address = $2
    )
    SELECT
        COALESCE(SUM(value_zat), 0)::BIGINT AS total_received_zat,
        COALESCE(SUM(value_zat) FILTER (WHERE unspent), 0)::BIGINT AS unspent_zat,
        COALESCE(SUM(value_zat) FILTER (
            WHERE unspent AND is_coinbase AND ($3 - height + 1) < $4
        ), 0)::BIGINT AS immature_coinbase_zat,
        COALESCE(SUM(value_zat) FILTER (
            WHERE unspent AND is_coinbase AND ($3 - height + 1) >= $4
        ), 0)::BIGINT AS mature_coinbase_zat,
        COALESCE(COUNT(*) FILTER (WHERE unspent), 0)::BIGINT AS utxo_count,
        COALESCE(COUNT(*) FILTER (WHERE is_coinbase), 0)::BIGINT AS mined_output_count
    FROM owned";

#[cfg(test)]
mod tests {
    use super::*;

    fn network() -> NetworkConfig {
        NetworkConfig {
            id: "testnet".to_owned(),
            display_name: "Wcash Testnet".to_owned(),
            symbol: "tWEC".to_owned(),
            decimals: 8,
            coinbase_maturity: 100,
            target_spacing_seconds: 75,
            max_supply_zat: 2_100_000_000_000_000,
            initial_subsidy_zat: 625_000_000,
            halving_interval: 1_680_000,
            first_halving_height: 1_680_001,
            genesis_hash: "00".repeat(32),
            parent_explorer_block_url: "https://example.invalid/block/{hash}".to_owned(),
            parent_explorer_tx_url: "https://example.invalid/tx/{txid}".to_owned(),
        }
    }

    #[test]
    fn amounts_are_exact_decimal_strings() {
        assert_eq!(amount(625_000_000, &network()).decimal, "6.25000000");
        assert_eq!(amount(-1, &network()).decimal, "-0.00000001");
        assert_eq!(amount(0, &network()).zatoshi, "0");
    }

    #[test]
    fn cursors_round_trip_and_reject_garbage() {
        let cursor = encode_cursor(42);
        assert_eq!(decode_cursor(&cursor).expect("valid cursor"), 42);
        assert!(decode_cursor("../../etc/passwd").is_err());
        let tx_cursor = encode_tx_cursor(42, 3);
        assert_eq!(
            decode_tx_cursor(&tx_cursor).expect("valid tx cursor"),
            (42, 3)
        );
    }

    #[test]
    fn halving_height_is_derived_from_consensus_config() {
        let network = network();
        assert_eq!(next_halving_height(0, &network), 1_680_001);
        assert_eq!(next_halving_height(1_680_001, &network), 3_360_001);
    }

    #[test]
    fn openapi_document_covers_public_routes_and_resolves_local_references() {
        fn check_references(root: &Value, value: &Value) {
            match value {
                Value::Array(values) => {
                    for value in values {
                        check_references(root, value);
                    }
                }
                Value::Object(object) => {
                    if let Some(reference) = object.get("$ref").and_then(Value::as_str) {
                        let pointer = reference
                            .strip_prefix('#')
                            .expect("only document-local OpenAPI references are used");
                        assert!(
                            root.pointer(pointer).is_some(),
                            "unresolved OpenAPI reference {reference}"
                        );
                    }
                    for value in object.values() {
                        check_references(root, value);
                    }
                }
                _ => {}
            }
        }

        let document = openapi_document();
        let paths = document["paths"].as_object().expect("paths object");
        let expected_paths = [
            "/health/live",
            "/health/ready",
            "/api/v1/status",
            "/api/v1/stats",
            "/api/v1/blocks",
            "/api/v1/blocks/{id}",
            "/api/v1/blocks/{id}/raw",
            "/api/v1/blocks/{id}/auxpow",
            "/api/v1/transactions",
            "/api/v1/transactions/{txid}",
            "/api/v1/addresses/{address}",
            "/api/v1/reorgs",
            "/api/v1/search",
            "/api/openapi.json",
        ];
        assert_eq!(paths.len(), expected_paths.len());
        for path in expected_paths {
            assert!(paths.contains_key(path), "missing OpenAPI path {path}");
        }

        check_references(&document, &document);
    }
}
