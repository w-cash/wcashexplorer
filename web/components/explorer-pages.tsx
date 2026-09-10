'use client';

import Link from 'next/link';
import {
  AlertTriangle,
  ArrowLeft,
  ArrowRight,
  Check,
  CircleGauge,
  Coins,
  ExternalLink,
  FileKey,
  GitMerge,
  Layers3,
  LockKeyhole,
  Network,
  Pickaxe,
  ShieldCheck,
  WalletCards,
} from 'lucide-react';
import { useEffect, useState } from 'react';
import {
  type AddressDetail,
  type BlockDetail,
  type BlockSummary,
  type Envelope,
  type Reorg,
  type Status,
  type TransactionDetail,
  type TransactionSummary,
  loadAddress,
  loadBlock,
  loadBlocks,
  loadReorgs,
  loadStatus,
  loadTransaction,
  loadTransactions,
  previewDashboard,
  previewEnabled,
} from '@/lib/explorer-data';
import {
  BlockTable,
  humanize,
  middleEllipsis,
  relativeTime,
  trimAmount,
} from './dashboard';
import { SearchBox } from './search-box';

type Resource<T> = {
  envelope: Envelope<T> | null;
  error: string;
  loading: boolean;
};

const emptyResource = <T,>(): Resource<T> => ({
  envelope: null,
  error: '',
  loading: true,
});

export function BlocksPage() {
  const [resource, setResource] =
    useState<Resource<BlockSummary[]>>(emptyResource);
  const [cursor, setCursor] = useState<string>();

  useEffect(() => {
    const controller = new AbortController();
    void loadBlocks(cursor, controller.signal).then(
      (envelope) => setResource({ envelope, error: '', loading: false }),
      () => {
        if (previewEnabled && !cursor) {
          setResource({
            envelope: previewDashboard.blocks,
            error: '',
            loading: false,
          });
        } else {
          setResource({
            envelope: null,
            error: 'The indexed block feed is unavailable.',
            loading: false,
          });
        }
      },
    );
    return () => controller.abort();
  }, [cursor]);

  return (
    <PageFrame
      eyebrow="Canonical chain"
      title="Wcash blocks"
      description="Only blocks selected by the indexed Wcash canonical chain appear here."
    >
      <SearchBox />
      {previewEnabled && resource.envelope === previewDashboard.blocks ? (
        <PreviewNotice />
      ) : null}
      <ResourceState loading={resource.loading} error={resource.error}>
        <BlockTable blocks={resource.envelope?.data ?? []} loading={false} />
        <Pagination
          canGoBack={Boolean(cursor)}
          canGoForward={Boolean(resource.envelope?.meta.nextCursor)}
          onBack={() => setCursor(undefined)}
          onForward={() =>
            setCursor(resource.envelope?.meta.nextCursor ?? undefined)
          }
        />
      </ResourceState>
    </PageFrame>
  );
}

export function TransactionsPage() {
  const [resource, setResource] =
    useState<Resource<TransactionSummary[]>>(emptyResource);
  const [cursor, setCursor] = useState<string>();

  useEffect(() => {
    const controller = new AbortController();
    void loadTransactions(cursor, controller.signal).then(
      (envelope) => setResource({ envelope, error: '', loading: false }),
      () =>
        setResource({
          envelope: null,
          error: 'The indexed transaction feed is unavailable.',
          loading: false,
        }),
    );
    return () => controller.abort();
  }, [cursor]);

  return (
    <PageFrame
      eyebrow="Canonical activity"
      title="Transactions"
      description="Public transaction structure is shown exactly; shielded identities and note values remain private."
    >
      <SearchBox />
      <ResourceState loading={resource.loading} error={resource.error}>
        <TransactionTable transactions={resource.envelope?.data ?? []} />
        <Pagination
          canGoBack={Boolean(cursor)}
          canGoForward={Boolean(resource.envelope?.meta.nextCursor)}
          onBack={() => setCursor(undefined)}
          onForward={() =>
            setCursor(resource.envelope?.meta.nextCursor ?? undefined)
          }
        />
      </ResourceState>
    </PageFrame>
  );
}

export function BlockPage({ id }: { id: string }) {
  const [resource, setResource] =
    useState<Resource<BlockDetail>>(emptyResource);

  useEffect(() => {
    const controller = new AbortController();
    void loadBlock(id, controller.signal).then(
      (envelope) => setResource({ envelope, error: '', loading: false }),
      () =>
        setResource({
          envelope: null,
          error: `Block ${id} was not found in the indexed canonical chain.`,
          loading: false,
        }),
    );
    return () => controller.abort();
  }, [id]);

  const block = resource.envelope?.data;
  return (
    <PageFrame
      eyebrow="Block record"
      title={block ? `Block #${block.height.toLocaleString()}` : 'Block'}
      description="Wcash identity, canonical witness, transactions, and merge-mining evidence are kept separate."
    >
      <ResourceState loading={resource.loading} error={resource.error}>
        {block ? <BlockDetailView block={block} /> : null}
      </ResourceState>
    </PageFrame>
  );
}

export function TransactionPage({
  txid,
  blockHash,
}: {
  txid: string;
  blockHash?: string;
}) {
  const [resource, setResource] =
    useState<Resource<TransactionDetail>>(emptyResource);

  useEffect(() => {
    const controller = new AbortController();
    void loadTransaction(txid, blockHash, controller.signal).then(
      (envelope) => setResource({ envelope, error: '', loading: false }),
      () =>
        setResource({
          envelope: null,
          error:
            'That transaction was not found on the indexed canonical chain.',
          loading: false,
        }),
    );
    return () => controller.abort();
  }, [blockHash, txid]);

  return (
    <PageFrame
      eyebrow="Transaction record"
      title="Transaction"
      description={middleEllipsis(txid, 34)}
    >
      <ResourceState loading={resource.loading} error={resource.error}>
        {resource.envelope ? (
          <TransactionDetailView transaction={resource.envelope.data} />
        ) : null}
      </ResourceState>
    </PageFrame>
  );
}

export function AddressPage({ address }: { address: string }) {
  const [resource, setResource] =
    useState<Resource<AddressDetail>>(emptyResource);

  useEffect(() => {
    const controller = new AbortController();
    void loadAddress(address, controller.signal).then(
      (envelope) => setResource({ envelope, error: '', loading: false }),
      () =>
        setResource({
          envelope: null,
          error: 'That transparent address was not found.',
          loading: false,
        }),
    );
    return () => controller.abort();
  }, [address]);

  return (
    <PageFrame
      eyebrow="Transparent scope"
      title="Address"
      description={middleEllipsis(address, 38)}
    >
      <ResourceState loading={resource.loading} error={resource.error}>
        {resource.envelope ? (
          <AddressDetailView address={resource.envelope.data} />
        ) : null}
      </ResourceState>
    </PageFrame>
  );
}

export function MergeMiningPage() {
  const [resource, setResource] =
    useState<Resource<BlockSummary[]>>(emptyResource);

  useEffect(() => {
    const controller = new AbortController();
    void loadBlocks(undefined, controller.signal).then(
      (envelope) => setResource({ envelope, error: '', loading: false }),
      () => {
        if (previewEnabled) {
          setResource({
            envelope: previewDashboard.blocks,
            error: '',
            loading: false,
          });
        } else {
          setResource({
            envelope: null,
            error:
              'Merge-mining evidence is unavailable while the indexer is offline.',
            loading: false,
          });
        }
      },
    );
    return () => controller.abort();
  }, []);

  const blocks = resource.envelope?.data ?? [];
  const canonicalParents = blocks.filter(
    (block) => block.mergeMining.parentLookupState === 'canonical',
  ).length;
  return (
    <PageFrame
      eyebrow="Zcash parent evidence"
      title="Merge mining"
      description="Every Wcash block proves its own AuxPoW validity. Parent-chain canonical acceptance is a separate, source-observed fact."
    >
      <div className="grid gap-4 md:grid-cols-3">
        <FactCard
          icon={<ShieldCheck />}
          label="Local proof"
          value="Full consensus validation"
        />
        <FactCard
          icon={<GitMerge />}
          label="Parent observations"
          value="Up to two Zcash nodes"
        />
        <FactCard
          icon={<Layers3 />}
          label="Canonical matches in view"
          value={`${canonicalParents} of ${blocks.length}`}
        />
      </div>
      <section className="panel p-6 sm:p-8">
        <h2 className="text-xl font-extrabold tracking-[-0.03em]">
          What the labels mean
        </h2>
        <div className="mt-6 grid gap-5 md:grid-cols-3">
          <Definition
            title="AuxPoW verified"
            body="The embedded Equihash solution, child commitment, Merkle branches, indices, and Wcash target passed the pinned consensus verifier."
          />
          <Definition
            title="Exact witness"
            body="The Wcash node recognizes this precise serialized AuxPoW witness—not only the witness-independent block ID."
          />
          <Definition
            title="Configured Zcash sources agree"
            body="Configured Zcash Testnet validators returned the exact parent block and its raw header matched the header embedded in the proof."
          />
        </div>
      </section>
      <ResourceState loading={resource.loading} error={resource.error}>
        <BlockTable blocks={blocks} loading={false} />
      </ResourceState>
    </PageFrame>
  );
}

export function NetworkPage() {
  const [status, setStatus] = useState<Resource<Status>>(emptyResource);
  const [reorgs, setReorgs] = useState<Resource<Reorg[]>>(emptyResource);

  useEffect(() => {
    const controller = new AbortController();
    void Promise.all([
      loadStatus(controller.signal),
      loadReorgs(controller.signal),
    ]).then(
      ([statusEnvelope, reorgEnvelope]) => {
        setStatus({ envelope: statusEnvelope, error: '', loading: false });
        setReorgs({ envelope: reorgEnvelope, error: '', loading: false });
      },
      () => {
        setStatus({
          envelope: null,
          error: 'Network telemetry is unavailable.',
          loading: false,
        });
        setReorgs({ envelope: null, error: '', loading: false });
      },
    );
    return () => controller.abort();
  }, []);

  const network = status.envelope?.data;
  return (
    <PageFrame
      eyebrow="Indexer telemetry"
      title="Network"
      description="Consensus constants, observed chain state, synchronization health, and reorganization history."
    >
      <ResourceState loading={status.loading} error={status.error}>
        {network ? (
          <>
            <div className="grid gap-4 sm:grid-cols-2 xl:grid-cols-4">
              <FactCard
                icon={<Network />}
                label="Indexed / node tip"
                value={`${network.indexedHeight ?? '—'} / ${network.nodeHeight ?? '—'}`}
              />
              <FactCard
                icon={<CircleGauge />}
                label="Target spacing"
                value={`${network.targetSpacingSeconds} seconds`}
              />
              <FactCard
                icon={<Coins />}
                label="Initial subsidy"
                value={`${trimAmount(network.initialSubsidy.decimal)} ${network.symbol}`}
              />
              <FactCard
                icon={<Pickaxe />}
                label="Next halving"
                value={`Block ${network.nextHalvingHeight.toLocaleString()}`}
              />
            </div>
            <section className="panel p-6 sm:p-8">
              <h2 className="text-xl font-extrabold tracking-[-0.03em]">
                Supply policy
              </h2>
              <dl className="record-grid mt-6">
                <Record
                  label="Maximum supply"
                  value={`${Number(network.maxSupply.decimal).toLocaleString()} ${network.symbol}`}
                  mono
                />
                <Record label="Atomic decimals" value="8" mono />
                <Record
                  label="Coinbase maturity"
                  value={`${network.coinbaseMaturity} blocks`}
                  mono
                />
                <Record
                  label="Observed spacing"
                  value={
                    network.observedSpacingSeconds
                      ? `${Math.round(network.observedSpacingSeconds)} seconds`
                      : 'Not enough data'
                  }
                  mono
                />
              </dl>
            </section>
          </>
        ) : null}
      </ResourceState>
      <section>
        <h2 className="mb-4 text-xl font-extrabold tracking-[-0.03em]">
          Canonical reorganization log
        </h2>
        {reorgs.envelope?.data.length ? (
          <div className="space-y-3">
            {reorgs.envelope.data.map((event) => (
              <article
                key={event.eventId}
                className="panel flex flex-wrap items-center justify-between gap-4 p-5 text-sm"
              >
                <div>
                  <div className="font-bold">
                    Old tip #{event.oldTipHeight.toLocaleString()}
                  </div>
                  <div className="hash mt-1 max-w-sm text-xs text-[var(--muted)]">
                    {event.oldTipHash}
                  </div>
                </div>
                <div className="text-right text-[var(--muted)]">
                  <div>Ancestor #{event.commonAncestorHeight ?? 'none'}</div>
                  <time dateTime={event.detectedAt}>
                    {relativeTime(event.detectedAt)}
                  </time>
                </div>
              </article>
            ))}
          </div>
        ) : (
          <div className="panel p-6 text-sm text-[var(--muted)]">
            No indexed reorganization events.
          </div>
        )}
      </section>
    </PageFrame>
  );
}

function BlockDetailView({ block }: { block: BlockDetail }) {
  const auxpow = block.auxpow;
  return (
    <div className="space-y-8">
      <div className="flex flex-wrap items-center gap-3">
        <Link href="/blocks" className="pill">
          <ArrowLeft size={13} /> All blocks
        </Link>
        {block.previousBlockHash ? (
          <Link href={`/block/${block.previousBlockHash}`} className="pill">
            <ArrowLeft size={13} /> Previous
          </Link>
        ) : null}
        {block.nextBlockHash ? (
          <Link href={`/block/${block.nextBlockHash}`} className="pill">
            Next <ArrowRight size={13} />
          </Link>
        ) : null}
        <a
          href={`/api/v1/blocks/${block.hash}/raw`}
          target="_blank"
          rel="noreferrer"
          className="pill"
        >
          Raw witness <ExternalLink size={13} />
        </a>
        <span className="pill pill-success">
          <Check size={13} /> Canonical · {block.confirmations} confirmations
        </span>
      </div>
      <section className="panel p-6 sm:p-8">
        <h2 className="text-xl font-extrabold tracking-[-0.03em]">
          Block identity
        </h2>
        <dl className="record-grid mt-6">
          <Record label="Block hash" value={block.hash} mono wide />
          <Record
            label="Exact witness hash"
            value={block.witnessHash}
            mono
            wide
          />
          <Record label="Mined at" value={relativeTime(block.time)} />
          <Record
            label="Serialized size"
            value={`${block.sizeBytes.toLocaleString()} bytes`}
            mono
          />
          <Record label="Difficulty" value={block.difficulty} mono />
          <Record label="Compact target" value={block.bits} mono />
          <Record
            label="Transactions"
            value={block.transactionCount.toString()}
            mono
          />
          <Record
            label="Issued subsidy"
            value={`${trimAmount(block.reward.decimal)} ${block.reward.symbol}`}
            mono
          />
          <Record label="Merkle root" value={block.merkleRoot} mono wide />
          <Record
            label="Block commitments"
            value={block.blockCommitments ?? 'Not present'}
            mono
            wide
          />
        </dl>
      </section>
      <section id="auxpow" className="panel scroll-mt-28 p-6 sm:p-8">
        <div className="flex flex-wrap items-start justify-between gap-4">
          <div>
            <div className="eyebrow">Proof ladder</div>
            <h2 className="mt-2 text-xl font-extrabold tracking-[-0.03em]">
              AuxPoW evidence
            </h2>
          </div>
          {auxpow ? (
            <span className="pill pill-success">
              <Check size={13} />{' '}
              {auxpow.localValidationState === 'auxpow_verified'
                ? 'AuxPoW verified'
                : humanize(auxpow.localValidationState)}
            </span>
          ) : (
            <span className="pill">Genesis work</span>
          )}
        </div>
        {auxpow ? (
          <div className="mt-7 space-y-6">
            <dl className="record-grid">
              <Record
                label="Exact Wcash witness"
                value={humanize(auxpow.exactWitnessState)}
              />
              <Record
                label="Proof format"
                value={`WCAZ v${auxpow.proofVersion}`}
                mono
              />
              <Record
                label="Proof size"
                value={`${auxpow.proofSize.toLocaleString()} bytes`}
                mono
              />
              <Record
                label="Verifier"
                value={auxpow.verifierVersion ?? 'Pinned verifier'}
                mono
              />
              <Record
                label="Parent hash meets header target"
                value={auxpow.parentHashMeetsClaimedTarget ? 'Yes' : 'No'}
              />
              <Record
                label="Parent-chain lookup"
                value={humanize(auxpow.parentLookupState)}
              />
              <Record
                label="Parent block hash"
                value={auxpow.parentBlockHash}
                mono
                wide
              />
              <Record
                label="Parent coinbase txid"
                value={auxpow.parentCoinbaseTxid}
                mono
                wide
              />
            </dl>
            <div className="grid gap-3 md:grid-cols-2">
              <ExternalEvidence
                href={auxpow.parentBlockUrl}
                label="Inspect parent block"
              />
              <ExternalEvidence
                href={auxpow.parentCoinbaseTxUrl}
                label="Inspect parent coinbase"
              />
            </div>
            <div>
              <h3 className="font-extrabold">Parent observations</h3>
              <div className="mt-3 space-y-3">
                {auxpow.observations.map((observation) => (
                  <article
                    key={observation.sourceName}
                    className="rounded-xl border border-[var(--border)] bg-[var(--raised)] p-4 text-sm"
                  >
                    <div className="flex flex-wrap items-center justify-between gap-2">
                      <span className="mono font-bold">
                        {observation.sourceName}
                      </span>
                      <span
                        className={
                          observation.observationState === 'canonical'
                            ? 'pill pill-success'
                            : 'pill pill-info'
                        }
                      >
                        {humanize(observation.observationState)}
                      </span>
                    </div>
                    <div className="mt-3 text-[var(--muted)]">
                      Embedded header match:{' '}
                      <strong className="text-[var(--text)]">
                        {observation.embeddedHeaderMatches === null
                          ? 'not checked'
                          : observation.embeddedHeaderMatches
                            ? 'yes'
                            : 'no'}
                      </strong>
                      {observation.parentHeight === null
                        ? ''
                        : ` · parent height ${observation.parentHeight.toLocaleString()}`}
                    </div>
                  </article>
                ))}
              </div>
            </div>
          </div>
        ) : (
          <p className="mt-5 text-sm leading-6 text-[var(--muted)]">
            Genesis uses its native committed work and has no auxiliary parent
            proof.
          </p>
        )}
      </section>
      <section>
        <h2 className="mb-4 text-xl font-extrabold tracking-[-0.03em]">
          Transactions
        </h2>
        <TransactionTable transactions={block.transactions} />
      </section>
      {block.valuePools.length ? (
        <section>
          <h2 className="mb-4 text-xl font-extrabold tracking-[-0.03em]">
            Public value-pool totals
          </h2>
          <div className="grid gap-3 sm:grid-cols-2 xl:grid-cols-3">
            {block.valuePools.map((pool) => (
              <FactCard
                key={pool.id}
                icon={<LockKeyhole />}
                label={humanize(pool.id)}
                value={
                  pool.chainValue
                    ? `${trimAmount(pool.chainValue.decimal)} ${pool.chainValue.symbol}`
                    : 'Not monitored'
                }
              />
            ))}
          </div>
        </section>
      ) : null}
    </div>
  );
}

function TransactionDetailView({
  transaction,
}: {
  transaction: TransactionDetail;
}) {
  const shieldedActions =
    transaction.saplingSpendCount +
    transaction.saplingOutputCount +
    transaction.orchardActionCount +
    transaction.ironwoodActionCount;
  return (
    <div className="space-y-8">
      <section className="panel p-6 sm:p-8">
        <div className="flex flex-wrap items-center justify-between gap-3">
          <span className="pill pill-info">{humanize(transaction.kind)}</span>
          <Link
            href={`/block/${transaction.blockHeight}`}
            className="text-sm font-bold text-[var(--brand)]"
          >
            Block #{transaction.blockHeight.toLocaleString()}
          </Link>
        </div>
        <dl className="record-grid mt-6">
          <Record label="Transaction ID" value={transaction.txid} mono wide />
          <Record
            label="Authorization digest"
            value={transaction.authDigest}
            mono
            wide
          />
          <Record label="Version" value={transaction.version.toString()} mono />
          <Record
            label="Serialized size"
            value={`${transaction.sizeBytes.toLocaleString()} bytes`}
            mono
          />
          <Record
            label="Public output total"
            value={`${trimAmount(transaction.publicOutputValue.decimal)} ${transaction.publicOutputValue.symbol}`}
            mono
          />
          <Record
            label="Fee"
            value={
              transaction.fee
                ? `${trimAmount(transaction.fee.decimal)} ${transaction.fee.symbol}`
                : 'Not available'
            }
            mono
          />
          <Record
            label="Transparent inputs / outputs"
            value={`${transaction.transparentInputCount} / ${transaction.transparentOutputCount}`}
            mono
          />
          <Record
            label="Shielded actions"
            value={shieldedActions.toString()}
            mono
          />
        </dl>
      </section>
      {shieldedActions > 0 ? (
        <PrivacyNotice text={transaction.privacyNotice} />
      ) : null}
      <section>
        <h2 className="mb-4 text-xl font-extrabold tracking-[-0.03em]">
          Transparent inputs
        </h2>
        <div className="space-y-3">
          {transaction.inputs.map((input) => (
            <article key={input.inputIndex} className="panel p-5 text-sm">
              <div className="eyebrow">Input {input.inputIndex}</div>
              <div className="hash mt-3">
                {input.coinbaseData
                  ? `Coinbase ${input.coinbaseData}`
                  : `${input.previousTxid}:${input.previousOutputIndex}`}
              </div>
            </article>
          ))}
        </div>
      </section>
      <section>
        <h2 className="mb-4 text-xl font-extrabold tracking-[-0.03em]">
          Transparent outputs
        </h2>
        <div className="space-y-3">
          {transaction.outputs.map((output) => (
            <article
              key={output.index}
              className="panel grid gap-3 p-5 text-sm sm:grid-cols-[100px_1fr_auto]"
            >
              <span className="eyebrow">Output {output.index}</span>
              <span className="hash">
                {output.address ?? output.scriptType ?? 'Non-address script'}
              </span>
              <span className="mono font-bold">
                {trimAmount(output.value.decimal)} {output.value.symbol}
              </span>
            </article>
          ))}
        </div>
      </section>
    </div>
  );
}

function AddressDetailView({ address }: { address: AddressDetail }) {
  return (
    <div className="space-y-8">
      <PrivacyNotice text={address.scopeNotice} />
      <div className="grid gap-4 sm:grid-cols-2 xl:grid-cols-4">
        <FactCard
          icon={<WalletCards />}
          label="Unspent"
          value={`${trimAmount(address.unspent.decimal)} ${address.unspent.symbol}`}
        />
        <FactCard
          icon={<Coins />}
          label="Total received"
          value={`${trimAmount(address.totalReceived.decimal)} ${address.totalReceived.symbol}`}
        />
        <FactCard
          icon={<Pickaxe />}
          label="Immature coinbase"
          value={`${trimAmount(address.immatureCoinbase.decimal)} ${address.immatureCoinbase.symbol}`}
        />
        <FactCard
          icon={<ShieldCheck />}
          label="Mature; must shield"
          value={`${trimAmount(address.matureCoinbaseMustShield.decimal)} ${address.matureCoinbaseMustShield.symbol}`}
        />
      </div>
      <section>
        <h2 className="mb-4 text-xl font-extrabold tracking-[-0.03em]">
          Latest received outputs
        </h2>
        <div className="table-shell">
          <table className="data-table">
            <caption className="sr-only">
              Latest transparent outputs received by this address
            </caption>
            <thead>
              <tr>
                <th>Block</th>
                <th>Transaction</th>
                <th>Received</th>
                <th>Time (UTC)</th>
              </tr>
            </thead>
            <tbody>
              {address.activity.map((activity) => (
                <tr
                  key={`${activity.txid}:${activity.authDigest}:${activity.blockHash}`}
                >
                  <td data-label="Block">
                    <Link
                      href={`/block/${activity.blockHeight}`}
                      className="mono text-[var(--brand)]"
                    >
                      #{activity.blockHeight}
                    </Link>
                  </td>
                  <td data-label="Transaction" data-wide="true">
                    <Link
                      href={`/tx/${activity.txid}?block=${activity.blockHash}`}
                      className="hash block"
                    >
                      {middleEllipsis(activity.txid, 24)}
                    </Link>
                    <span className="sr-only">
                      {' '}
                      Authorization digest {activity.authDigest}
                    </span>
                  </td>
                  <td data-label="Received" className="mono">
                    {trimAmount(activity.received.decimal)}{' '}
                    {activity.received.symbol}
                  </td>
                  <td data-label="Time (UTC)">
                    <time dateTime={activity.blockTime}>
                      {relativeTime(activity.blockTime)}
                    </time>
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      </section>
    </div>
  );
}

function TransactionTable({
  transactions,
}: {
  transactions: TransactionSummary[];
}) {
  if (!transactions.length)
    return (
      <div className="panel p-6 text-sm text-[var(--muted)]">
        No canonical transactions are indexed here.
      </div>
    );
  return (
    <div className="table-shell">
      <table className="data-table">
        <caption className="sr-only">
          Canonical Wcash transaction instances
        </caption>
        <thead>
          <tr>
            <th>Transaction ID</th>
            <th>Kind</th>
            <th>Block</th>
            <th>Public output</th>
            <th>Shielded actions</th>
            <th>Time (UTC)</th>
          </tr>
        </thead>
        <tbody>
          {transactions.map((transaction) => {
            const shielded =
              transaction.saplingSpendCount +
              transaction.saplingOutputCount +
              transaction.orchardActionCount +
              transaction.ironwoodActionCount;
            return (
              <tr key={`${transaction.txid}:${transaction.authDigest}`}>
                <td data-label="Transaction ID" data-wide="true">
                  <Link
                    href={`/tx/${transaction.txid}?block=${transaction.blockHash}`}
                    className="hash block"
                  >
                    {middleEllipsis(transaction.txid, 26)}
                  </Link>
                  <span className="sr-only">
                    {' '}
                    Authorization digest {transaction.authDigest}
                  </span>
                </td>
                <td data-label="Kind">
                  <span className="pill pill-info">
                    {humanize(transaction.kind)}
                  </span>
                </td>
                <td data-label="Block">
                  <Link
                    href={`/block/${transaction.blockHeight}`}
                    className="mono text-[var(--brand)]"
                  >
                    #{transaction.blockHeight.toLocaleString()}
                  </Link>
                </td>
                <td data-label="Public output" className="mono">
                  {trimAmount(transaction.publicOutputValue.decimal)}{' '}
                  {transaction.publicOutputValue.symbol}
                </td>
                <td data-label="Shielded actions" className="mono">
                  {shielded}
                </td>
                <td data-label="Time (UTC)">
                  <time dateTime={transaction.blockTime}>
                    {relativeTime(transaction.blockTime)}
                  </time>
                </td>
              </tr>
            );
          })}
        </tbody>
      </table>
    </div>
  );
}

function PageFrame({
  eyebrow,
  title,
  description,
  children,
}: {
  eyebrow: string;
  title: string;
  description: string;
  children: React.ReactNode;
}) {
  return (
    <div className="space-y-8">
      <header className="max-w-4xl">
        <div className="eyebrow">{eyebrow}</div>
        <h1 className="mt-3 text-[clamp(2.2rem,5vw,4.8rem)] font-extrabold leading-none tracking-[-0.06em]">
          {title}
        </h1>
        <p className="mt-5 max-w-3xl text-base leading-7 text-[var(--muted)] sm:text-lg">
          {description}
        </p>
      </header>
      {children}
    </div>
  );
}

function ResourceState({
  loading,
  error,
  children,
}: {
  loading: boolean;
  error: string;
  children: React.ReactNode;
}) {
  if (loading)
    return (
      <output
        className="skeleton block h-72"
        aria-label="Loading indexed chain data"
      >
        <span className="sr-only">Loading indexed chain data</span>
      </output>
    );
  if (error)
    return (
      <div
        role="alert"
        className="panel flex items-start gap-3 border-[var(--danger)] p-6"
      >
        <AlertTriangle className="mt-0.5 shrink-0 text-[var(--danger)]" />
        <div>
          <div className="font-extrabold">Explorer data unavailable</div>
          <p className="mt-2 text-sm leading-6 text-[var(--muted)]">
            {error} No chain facts are substituted with sample data.
          </p>
        </div>
      </div>
    );
  return <>{children}</>;
}

function PreviewNotice() {
  return (
    <div className="panel flex items-start gap-3 border-[var(--warning)] p-4 text-sm">
      <AlertTriangle className="shrink-0 text-[var(--warning)]" size={18} />
      <p>
        <strong>Local interface preview.</strong> These fixed snapshots are
        visibly labeled and are never enabled by default in production.
      </p>
    </div>
  );
}

function Pagination({
  canGoBack,
  canGoForward,
  onBack,
  onForward,
}: {
  canGoBack: boolean;
  canGoForward: boolean;
  onBack: () => void;
  onForward: () => void;
}) {
  if (!canGoBack && !canGoForward) return null;
  return (
    <nav aria-label="Pagination" className="mt-5 flex justify-end gap-3">
      <button
        type="button"
        disabled={!canGoBack}
        onClick={onBack}
        className="pill disabled:opacity-40"
      >
        <ArrowLeft size={13} /> Newest
      </button>
      <button
        type="button"
        disabled={!canGoForward}
        onClick={onForward}
        className="pill disabled:opacity-40"
      >
        Older <ArrowRight size={13} />
      </button>
    </nav>
  );
}

function FactCard({
  icon,
  label,
  value,
}: {
  icon: React.ReactNode;
  label: string;
  value: string;
}) {
  return (
    <article className="panel min-w-0 p-5">
      <div className="flex items-center justify-between gap-3 text-[var(--muted)]">
        <span className="text-xs font-bold uppercase tracking-[0.08em]">
          {label}
        </span>
        <span className="text-[var(--brand)]">{icon}</span>
      </div>
      <div className="mono mt-4 break-words text-lg font-extrabold">
        {value}
      </div>
    </article>
  );
}

function Definition({ title, body }: { title: string; body: string }) {
  return (
    <div>
      <h3 className="font-extrabold">{title}</h3>
      <p className="mt-2 text-sm leading-6 text-[var(--muted)]">{body}</p>
    </div>
  );
}

function Record({
  label,
  value,
  mono = false,
  wide = false,
}: {
  label: string;
  value: string;
  mono?: boolean;
  wide?: boolean;
}) {
  return (
    <div className={wide ? 'record-wide min-w-0' : 'min-w-0'}>
      <dt className="text-xs font-bold uppercase tracking-[0.08em] text-[var(--faint)]">
        {label}
      </dt>
      <dd
        className={`${mono ? 'mono' : ''} mt-2 break-all text-sm font-semibold leading-6`}
      >
        {value}
      </dd>
    </div>
  );
}

function ExternalEvidence({ href, label }: { href: string; label: string }) {
  return (
    <a
      href={href}
      target="_blank"
      rel="noreferrer"
      className="flex min-h-12 items-center justify-center gap-2 rounded-xl border border-[var(--border-strong)] text-sm font-bold hover:bg-[var(--raised)]"
    >
      {label}
      <ExternalLink size={15} />
    </a>
  );
}

function PrivacyNotice({ text }: { text: string }) {
  return (
    <aside className="panel flex items-start gap-4 border-[var(--mint)] p-5">
      <FileKey className="mt-0.5 shrink-0 text-[var(--mint)]" />
      <div>
        <div className="font-extrabold">Privacy boundary</div>
        <p className="mt-2 text-sm leading-6 text-[var(--muted)]">{text}</p>
      </div>
    </aside>
  );
}
