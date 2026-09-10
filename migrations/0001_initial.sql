CREATE TABLE networks (
    network_id TEXT PRIMARY KEY,
    display_name TEXT NOT NULL,
    symbol TEXT NOT NULL,
    decimals SMALLINT NOT NULL CHECK (decimals BETWEEN 0 AND 18),
    coinbase_maturity BIGINT NOT NULL CHECK (coinbase_maturity >= 0),
    target_spacing_seconds BIGINT NOT NULL CHECK (target_spacing_seconds > 0),
    max_supply_zat BIGINT NOT NULL CHECK (max_supply_zat > 0),
    initial_subsidy_zat BIGINT NOT NULL CHECK (initial_subsidy_zat > 0),
    halving_interval BIGINT NOT NULL CHECK (halving_interval > 0),
    first_halving_height BIGINT NOT NULL CHECK (first_halving_height > 0),
    genesis_hash TEXT NOT NULL CHECK (genesis_hash ~ '^[0-9a-f]{64}$'),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE blocks (
    block_hash TEXT PRIMARY KEY CHECK (block_hash ~ '^[0-9a-f]{64}$'),
    network_id TEXT NOT NULL REFERENCES networks(network_id),
    height BIGINT NOT NULL CHECK (height >= 0),
    previous_block_hash TEXT CHECK (previous_block_hash IS NULL OR previous_block_hash ~ '^[0-9a-f]{64}$'),
    merkle_root TEXT NOT NULL CHECK (merkle_root ~ '^[0-9a-f]{64}$'),
    block_commitments TEXT CHECK (block_commitments IS NULL OR block_commitments ~ '^[0-9a-f]{64}$'),
    block_time TIMESTAMPTZ NOT NULL,
    size_bytes BIGINT NOT NULL CHECK (size_bytes > 0),
    transaction_count INTEGER NOT NULL CHECK (transaction_count >= 0),
    bits TEXT NOT NULL CHECK (bits ~ '^[0-9a-f]{8}$'),
    difficulty_text TEXT NOT NULL,
    nonce TEXT NOT NULL,
    chain_supply_zat BIGINT,
    first_seen_at TIMESTAMPTZ NOT NULL,
    raw_rpc JSONB NOT NULL
);

CREATE INDEX blocks_network_height_idx ON blocks (network_id, height DESC);
CREATE INDEX blocks_time_idx ON blocks (block_time DESC);

CREATE TABLE block_witnesses (
    block_hash TEXT NOT NULL REFERENCES blocks(block_hash),
    witness_hash TEXT NOT NULL CHECK (witness_hash ~ '^[0-9a-f]{64}$'),
    raw_block BYTEA NOT NULL,
    auxpow_bytes BYTEA,
    exact_witness_state TEXT NOT NULL,
    witness_confirmations BIGINT,
    local_validation_state TEXT NOT NULL,
    verifier_version TEXT,
    indexed_at TIMESTAMPTZ NOT NULL,
    PRIMARY KEY (block_hash, witness_hash),
    CHECK (octet_length(raw_block) > 0),
    CHECK (witness_confirmations IS NULL OR witness_confirmations >= 0)
);

CREATE TABLE canonical_chain (
    network_id TEXT NOT NULL REFERENCES networks(network_id),
    height BIGINT NOT NULL CHECK (height >= 0),
    block_hash TEXT NOT NULL,
    witness_hash TEXT NOT NULL,
    selected_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (network_id, height),
    UNIQUE (network_id, block_hash),
    FOREIGN KEY (block_hash, witness_hash)
        REFERENCES block_witnesses(block_hash, witness_hash)
);

CREATE TABLE transaction_instances (
    transaction_instance_id BIGSERIAL PRIMARY KEY,
    txid TEXT NOT NULL CHECK (txid ~ '^[0-9a-f]{64}$'),
    auth_digest TEXT NOT NULL CHECK (auth_digest ~ '^[0-9a-f]{64}$'),
    raw_hash TEXT NOT NULL CHECK (raw_hash ~ '^[0-9a-f]{64}$'),
    raw_transaction BYTEA,
    version BIGINT NOT NULL,
    size_bytes BIGINT NOT NULL CHECK (size_bytes > 0),
    lock_time BIGINT NOT NULL CHECK (lock_time >= 0),
    expiry_height BIGINT NOT NULL CHECK (expiry_height >= 0),
    is_coinbase BOOLEAN NOT NULL,
    fee_zat BIGINT,
    value_balance_zat BIGINT,
    sapling_spend_count INTEGER NOT NULL CHECK (sapling_spend_count >= 0),
    sapling_output_count INTEGER NOT NULL CHECK (sapling_output_count >= 0),
    orchard_action_count INTEGER NOT NULL CHECK (orchard_action_count >= 0),
    ironwood_action_count INTEGER NOT NULL CHECK (ironwood_action_count >= 0),
    raw_rpc JSONB NOT NULL,
    UNIQUE (txid, auth_digest)
);

CREATE INDEX transaction_instances_txid_idx ON transaction_instances (txid);

CREATE TABLE block_transactions (
    block_hash TEXT NOT NULL,
    witness_hash TEXT NOT NULL,
    tx_position INTEGER NOT NULL CHECK (tx_position >= 0),
    transaction_instance_id BIGINT NOT NULL REFERENCES transaction_instances(transaction_instance_id),
    PRIMARY KEY (block_hash, witness_hash, tx_position),
    UNIQUE (block_hash, witness_hash, transaction_instance_id),
    FOREIGN KEY (block_hash, witness_hash)
        REFERENCES block_witnesses(block_hash, witness_hash)
);

CREATE INDEX block_transactions_instance_idx
    ON block_transactions (transaction_instance_id);

CREATE TABLE transparent_outputs (
    transaction_instance_id BIGINT NOT NULL REFERENCES transaction_instances(transaction_instance_id),
    output_index INTEGER NOT NULL CHECK (output_index >= 0),
    value_zat BIGINT NOT NULL CHECK (value_zat >= 0),
    address TEXT,
    script_type TEXT,
    script_hex TEXT,
    PRIMARY KEY (transaction_instance_id, output_index)
);

CREATE INDEX transparent_outputs_address_idx
    ON transparent_outputs (address) WHERE address IS NOT NULL;

CREATE TABLE transparent_inputs (
    transaction_instance_id BIGINT NOT NULL REFERENCES transaction_instances(transaction_instance_id),
    input_index INTEGER NOT NULL CHECK (input_index >= 0),
    previous_txid TEXT CHECK (previous_txid IS NULL OR previous_txid ~ '^[0-9a-f]{64}$'),
    previous_output_index INTEGER CHECK (previous_output_index IS NULL OR previous_output_index >= 0),
    coinbase_data TEXT,
    sequence BIGINT CHECK (sequence IS NULL OR sequence BETWEEN 0 AND 4294967295),
    PRIMARY KEY (transaction_instance_id, input_index),
    CHECK (
        (coinbase_data IS NOT NULL AND previous_txid IS NULL AND previous_output_index IS NULL)
        OR
        (coinbase_data IS NULL AND previous_txid IS NOT NULL AND previous_output_index IS NOT NULL)
    )
);

CREATE INDEX transparent_inputs_prevout_idx
    ON transparent_inputs (previous_txid, previous_output_index)
    WHERE previous_txid IS NOT NULL;

CREATE TABLE value_pool_snapshots (
    block_hash TEXT NOT NULL,
    witness_hash TEXT NOT NULL,
    pool_id TEXT NOT NULL,
    chain_value_zat BIGINT,
    value_delta_zat BIGINT,
    monitored BOOLEAN,
    PRIMARY KEY (block_hash, witness_hash, pool_id),
    FOREIGN KEY (block_hash, witness_hash)
        REFERENCES block_witnesses(block_hash, witness_hash)
);

CREATE TABLE auxpow_links (
    block_hash TEXT NOT NULL,
    witness_hash TEXT NOT NULL,
    proof_version SMALLINT NOT NULL CHECK (proof_version >= 0),
    proof_size BIGINT NOT NULL CHECK (proof_size > 0),
    parent_block_hash TEXT NOT NULL CHECK (parent_block_hash ~ '^[0-9a-f]{64}$'),
    parent_header_bits TEXT NOT NULL CHECK (parent_header_bits ~ '^[0-9a-f]{8}$'),
    parent_hash_meets_claimed_target BOOLEAN NOT NULL,
    parent_coinbase_txid TEXT NOT NULL CHECK (parent_coinbase_txid ~ '^[0-9a-f]{64}$'),
    parent_merkle_depth INTEGER NOT NULL,
    parent_coinbase_index BIGINT NOT NULL,
    auth_data_merkle_depth INTEGER NOT NULL,
    auth_data_coinbase_index BIGINT NOT NULL,
    auxiliary_merkle_depth INTEGER NOT NULL,
    auxiliary_index BIGINT NOT NULL,
    parent_lookup_state TEXT NOT NULL,
    parent_sources_agree BOOLEAN NOT NULL,
    verified_at TIMESTAMPTZ NOT NULL,
    PRIMARY KEY (block_hash, witness_hash),
    FOREIGN KEY (block_hash, witness_hash)
        REFERENCES block_witnesses(block_hash, witness_hash)
);

CREATE INDEX auxpow_parent_hash_idx ON auxpow_links (parent_block_hash);

CREATE TABLE parent_chain_observations (
    block_hash TEXT NOT NULL,
    witness_hash TEXT NOT NULL,
    source_name TEXT NOT NULL,
    observation_state TEXT NOT NULL,
    parent_height BIGINT,
    parent_confirmations BIGINT,
    parent_time TIMESTAMPTZ,
    parent_bits TEXT,
    parent_difficulty_text TEXT,
    embedded_header_matches BOOLEAN,
    checked_at TIMESTAMPTZ NOT NULL,
    PRIMARY KEY (block_hash, witness_hash, source_name),
    FOREIGN KEY (block_hash, witness_hash)
        REFERENCES block_witnesses(block_hash, witness_hash)
);

CREATE TABLE chain_state (
    network_id TEXT PRIMARY KEY REFERENCES networks(network_id),
    indexed_height BIGINT,
    indexed_hash TEXT,
    indexed_witness_hash TEXT,
    node_height BIGINT,
    node_hash TEXT,
    status TEXT NOT NULL,
    last_error TEXT,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE reorg_events (
    event_id UUID PRIMARY KEY,
    network_id TEXT NOT NULL REFERENCES networks(network_id),
    old_tip_height BIGINT NOT NULL,
    old_tip_hash TEXT NOT NULL,
    common_ancestor_height BIGINT,
    common_ancestor_hash TEXT,
    detected_at TIMESTAMPTZ NOT NULL,
    completed_at TIMESTAMPTZ
);

CREATE INDEX reorg_events_network_time_idx
    ON reorg_events (network_id, detected_at DESC);
