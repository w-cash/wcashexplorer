-- ZIP-244 authorization digests exist only for transaction formats that
-- define them. Keep the database's internal instance key separate so legacy
-- transactions (including the Wcash genesis coinbase) are not assigned a
-- fabricated authorization digest.
ALTER TABLE transaction_instances
    RENAME COLUMN auth_digest TO instance_digest;

ALTER TABLE transaction_instances
    ADD COLUMN auth_digest TEXT
        CHECK (auth_digest IS NULL OR auth_digest ~ '^[0-9a-f]{64}$');

UPDATE transaction_instances
SET auth_digest = CASE
    WHEN version >= 5
         AND raw_rpc ? 'authdigest'
         AND (raw_rpc ->> 'authdigest') ~ '^[0-9a-f]{64}$'
    THEN raw_rpc ->> 'authdigest'
    ELSE NULL
END;

-- The old schema used either the RPC authorization digest or `raw_hash` in
-- this column. Normalize legacy versions to the explorer fingerprint and
-- modern versions to the consensus authorization digest.
UPDATE transaction_instances
SET instance_digest = CASE
    WHEN version >= 5 THEN COALESCE(auth_digest, raw_hash)
    ELSE raw_hash
END;

-- Abort the migration rather than silently blessing a previously indexed
-- modern transaction for which the node omitted consensus identity data.
ALTER TABLE transaction_instances
    ADD CONSTRAINT transaction_instances_auth_digest_version_check
        CHECK (
            (version < 5 AND auth_digest IS NULL)
            OR (version >= 5 AND auth_digest IS NOT NULL)
        );

ALTER TABLE transaction_instances
    DROP CONSTRAINT transaction_instances_txid_auth_digest_key;

ALTER TABLE transaction_instances
    ADD CONSTRAINT transaction_instances_txid_instance_digest_key
        UNIQUE (txid, instance_digest);

CREATE INDEX transaction_instances_auth_digest_idx
    ON transaction_instances (auth_digest)
    WHERE auth_digest IS NOT NULL;
