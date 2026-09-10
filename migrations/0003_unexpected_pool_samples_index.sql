-- Keep the public value-pool anomaly check proportional to actual anomalies.
-- The explorer intentionally accepts only transparent and Ironwood pool state.
CREATE INDEX value_pool_snapshots_unexpected_idx
    ON value_pool_snapshots (block_hash, witness_hash)
    WHERE pool_id NOT IN ('transparent', 'ironwood')
      AND (
          monitored IS TRUE
          OR COALESCE(chain_value_zat, 0) <> 0
          OR COALESCE(value_delta_zat, 0) <> 0
      );
