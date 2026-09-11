# Privacy Model

## Principle

WcashExplorer displays facts that are public on the Wcash chain and refuses to
invent facts hidden by the protocol. An explorer cannot turn shielded chain data
into a list of private addresses or balances without viewing keys or other
off-chain information, and this service accepts neither.

## What the explorer can show

For transparent activity, the explorer can index and display:

- public addresses and output scripts;
- public input references and output values;
- received-output activity and UTXO-derived balances;
- coinbase status and maturity according to the configured Wcash rules; and
- links between transparent previous outputs and later public spends.

For shielded activity, it can display only public protocol artifacts exposed by
the node, such as:

- counts of Sapling spends and outputs;
- counts of Orchard or Ironwood actions;
- aggregate chain value and per-block value-pool movement, when the node reports
  those values;
- commitments, nullifiers, ciphertexts, anchors, or other public transaction
  fields present in the raw node response; and
- the transaction's public metadata and block context.

These artifacts can prove that shielded state changed. They do not identify a
shielded address or reveal a note's plaintext.

## What it cannot show

Without a holder-supplied viewing capability, the explorer cannot truthfully
show:

- shielded sender or recipient addresses;
- a shielded account or unified-address balance;
- input-to-output linkage inside a shielded pool;
- individual shielded note values;
- transaction memos;
- the miner or pool receiving a shielded coinbase reward; or
- whether two shielded actions belong to the same person.

No API response or interface label should imply otherwise. “Shielded” means the
public transaction contains shielded protocol activity; it is not an assertion
about a known party.

## Address pages are transparent-only

Address lookup is intentionally limited to syntactically valid public
transparent addresses found in transparent outputs. The address balance is a
chain-derived transparent UTXO view, not a wallet balance and not a unified
address balance.

When a transparent coinbase output matures, the explorer may label the amount
according to Wcash's consensus spend policy. Once value enters the Ironwood
shielded pool, it is no longer attributable to that transparent address by this
explorer. The explorer must not follow later shielded value by guesswork.

Shielded and unified addresses are not accepted as search keys. Adding
viewing-key support would create a materially different product with custody,
retention, authorization, and breach risks; it is outside this explorer's scope.

## Transaction identity and metadata

The explorer preserves the transaction ID, an internal instance fingerprint,
and the ZIP-244 authorization digest when the transaction version defines one,
so distinct serialized or authorized instances are not collapsed. It never
labels an explorer-generated fingerprint as a consensus authorization digest.
Raw node JSON may contain public shielded artifacts, but never decrypted note
plaintext.

Network metadata still has privacy implications. Public access logs can reveal
which block, transaction, or transparent address a visitor requested. Operators
should therefore:

- avoid third-party analytics, advertising pixels, and unnecessary external
  assets;
- minimize and rotate HTTP logs;
- avoid logging full query strings where possible;
- set a short, documented retention period;
- restrict staff access to logs and database snapshots; and
- publish a separate privacy notice before operating a public service.

This repository does not currently define a production log-retention policy.

## Aggregate supply and value pools

The displayed “total issued” value comes from chain supply reported for the
canonical tip. It is not a sum of visible transparent balances. Value-pool totals
are protocol-level aggregates, not lists of users or accounts. A non-null value
reported by the node is exact, including zero. A missing value is shown as
unavailable. The node's legacy `monitored` flag is retained as compatibility
telemetry but is not used as a value-presence signal because current Wcash nodes
set it to false for an exactly zero pool.

## AuxPoW does not weaken transaction privacy by itself

AuxPoW proof data connects a Wcash block to a public Zcash parent block and
coinbase commitment. That linkage is public mining evidence. It does not reveal
the hidden recipients or values of Wcash shielded transactions. Pool payout
systems and network logs may create separate metadata, but they are outside the
explorer's on-chain view.

## Honest interface language

Preferred labels are factual and scoped:

- “transparent output value,” not “amount sent” when shielded components exist;
- “shielded action count,” not “private recipients”;
- “configured parent observations agree,” not “globally confirmed”;
- “local AuxPoW validation passed,” not an unqualified “verified”; and
- “total issued,” not “circulating supply.”

If a value cannot be derived from public consensus data, show it as unknown or
unavailable.
