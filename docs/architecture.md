# Architecture

## Purpose and trust boundary

WcashExplorer is a read model for Wcash Testnet. It does not participate in
consensus and does not replace validation by a Wcash or Zcash node. Its job is
to preserve the exact public data it receives, independently validate the
Wcash AuxPoW envelope, track canonical selection safely through reorganizations,
and state precisely which external observations support each claim.

```text
                         private network
  Wcash Testnet RPC ─┐
                     ├─> Rust indexer / AuxPoW verifier ─> PostgreSQL
  Zcash Testnet RPC A ┤                                  (authority)
  Zcash Testnet RPC B ┘                                      │
                                                            │ read-only
  Browser ─> TLS proxy ─> web interface ───── /api/v1 ─> Axum API
```

Node RPC and PostgreSQL belong behind the trust boundary. Only the web interface
and read-only explorer API are intended for public ingress.

## Components

### Indexer

The asynchronous Rust indexer polls one configured Wcash Testnet node. Before
indexing, it checks the node's chain name and genesis hash. It also validates
the configured Zcash parent nodes against the expected Zcash Testnet chain name
and genesis hash.

One process owns a PostgreSQL advisory lock for the singleton writer role. A
monitor checks that the lock session is still alive. Loss of the writer lease
stops the indexer instead of allowing an uncoordinated second writer.

For each height, the indexer obtains the expected hash, raw block bytes, and
verbose block representation. It checks height, hash, predecessor, and a stable
tip before publication. Raw and verbose forms are fetched together and the
solution bytes parsed from the raw block must exactly match the verbose solution.

### AuxPoW verifier

Non-genesis Wcash blocks must carry the WCAZ proof marker. The verifier decodes
the proof with the pinned `wcash-zcash-aux` revision and validates the commitment
to the child block and the required Wcash target. It records the parent header,
parent coinbase transaction ID, branch depths and indexes, advertised parent
target check, proof version, and proof size.

The resulting evidence has three separate layers:

| Layer | Question answered | Authority and limitation |
| --- | --- | --- |
| Local validation | Is this serialized AuxPoW proof internally valid for this Wcash block and target? | Deterministic pinned verifier; does not say the parent is canonical. |
| Exact Wcash witness | Does the configured Wcash node report this exact block hash plus solution as `best_chain`? | One configured Wcash node; the explorer refuses non-genesis blocks without this state. |
| Parent observation | Does a configured Zcash Testnet node return the exact parent hash, and do its raw header bytes equal the header embedded in the proof? | A time-stamped observation from that source, not global finality. |

The aggregate parent state distinguishes `canonical`, `orphaned`, `not_found`,
`unavailable`, `not_configured`, and `disagreement`. `parentSourcesAgree` means
the configured observations have the same state. Even with distinct URLs, they
may share an operator, network path, or implementation, so this is not a
Byzantine quorum or a cryptographic finality proof.

Wcash's block identifier does not uniquely identify the attached AuxPoW witness.
The explorer therefore computes a domain-separated witness hash over the raw
block and keys witness-dependent records by `(blockHash, witnessHash)`.

### PostgreSQL model

PostgreSQL is the sole authority for indexed state. Major table groups are:

- `networks` for immutable network identity and monetary parameters;
- `blocks`, `block_witnesses`, and `transaction_instances` for immutable facts;
- `canonical_chain` for the currently selected block and witness at each height;
- `block_transactions`, transparent inputs/outputs, and value-pool snapshots;
- `auxpow_links` and `parent_chain_observations` for proof and observation data;
- `chain_state` for readiness and node/indexer tips; and
- `reorg_events` for detected and completed canonical replacements.

Network identity is bound on first initialization. Later startup fails if the
configured genesis, symbol, decimals, maturity, spacing, subsidy, halving, or
supply cap conflicts with the stored row.

ZIP-244 transaction identity requires more than a transaction ID because
authorizing data is not committed in the same way. The schema stores unique
transaction instances by `(txid, auth_digest)` and associates the instance with
an exact block witness.

### Reorganizations

On divergence, the indexer searches backward for a common ancestor within
`MAX_REORG_DEPTH`. It fetches and validates the complete replacement branch,
then rechecks the observed Wcash snapshot. Only after the branch is staged does
one PostgreSQL transaction:

1. lock and compare the previously known tip;
2. insert immutable replacement facts;
3. record a reorganization event;
4. replace canonical selectors above the common ancestor;
5. update chain state; and
6. mark the event complete.

Readers therefore see either the old canonical branch or the fully published
replacement, never an intentionally half-published branch. Facts from displaced
blocks may remain for auditability even though public list queries select only
the canonical chain.

### Evidence refresh

Parent confirmations and canonical status are observations, not immutable block
facts. The indexer periodically revisits the stalest AuxPoW record inside a
bounded recent window. This updates the exact-witness and parent observations
without selecting a different Wcash block. Reorg detection remains the only
path that changes the canonical selector.

### Read-only API

Axum exposes bounded, cursor-paginated reads. Monetary amounts are emitted both
in atomic units and fixed-precision decimal strings. The API never proxies node
RPC. It applies a request-body limit, concurrency bound, response compression,
request IDs, panic containment, and restrictive response headers.

`/health/live` answers when the process can serve HTTP. `/health/ready` is
stricter: it requires a `ready` indexer state, a heartbeat no older than 30
seconds, present node and index tips, no more than one block of lag, matching
hashes at equal height, and a canonical selector matching the recorded tip.

### Web interface

The Vinext/React interface consumes the read-only API. In production, the
recommended topology serves the UI and forwards `/api/*` to the Rust service
under the same trusted origin. Preview data is development-only and must not be
enabled in a public deployment.

## Why PostgreSQL and not Redis

Canonical selection, reorg publication, foreign-key integrity, exact monetary
values, and transaction-instance identity benefit from PostgreSQL transactions
and constraints. Redis would add a second consistency model and create cache
invalidation hazards precisely when a reorg occurs.

No Redis dependency is planned until production measurements demonstrate a
specific bottleneck. If introduced later, it must be disposable, versioned by
canonical tip or witness identity, and safe to flush at any time. PostgreSQL
remains authoritative.

## Failure model

- **Wcash RPC unavailable:** indexing stops making progress and readiness ages
  out; existing reads can still be served but must not be presented as fresh.
- **One parent RPC unavailable:** the per-source state records the failure;
  aggregate agreement can no longer be claimed.
- **Parent disagreement:** the block's local proof may remain valid while the
  parent observation is explicitly `disagreement`.
- **PostgreSQL unavailable or writer lease lost:** the indexer exits or reports
  an error; an operator must restore the database and restart it.
- **Deep Wcash reorg:** a divergence beyond the configured bound is rejected for
  operator investigation rather than silently rewriting an unbounded history.
- **Tip changes during fetch:** the staged work is discarded and retried.

These behaviors make uncertainty visible. They do not make the explorer itself
a trusted consensus service.
