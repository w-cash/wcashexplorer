-- Cover the canonical UTXO, address activity, and leaderboard queries without
-- duplicating reorg-sensitive balances in rollup tables.
CREATE INDEX transparent_outputs_address_cover_idx
    ON transparent_outputs (address, transaction_instance_id, output_index)
    INCLUDE (value_zat)
    WHERE address IS NOT NULL;

CREATE INDEX transparent_inputs_prevout_cover_idx
    ON transparent_inputs (previous_txid, previous_output_index)
    INCLUDE (transaction_instance_id)
    WHERE previous_txid IS NOT NULL;

CREATE INDEX transaction_instances_txid_cover_idx
    ON transaction_instances (txid, transaction_instance_id)
    INCLUDE (auth_digest, is_coinbase);

CREATE INDEX value_pool_snapshots_pool_cover_idx
    ON value_pool_snapshots (pool_id, block_hash, witness_hash)
    INCLUDE (chain_value_zat, value_delta_zat, monitored);

CREATE INDEX parent_observations_state_cover_idx
    ON parent_chain_observations (block_hash, witness_hash, observation_state)
    INCLUDE (source_name, embedded_header_matches);
