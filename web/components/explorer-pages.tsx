'use client';

import Link from 'next/link';
import {
  AlertTriangle,
  ArrowLeft,
  ArrowRight,
  ExternalLink,
} from 'lucide-react';
import { useEffect, useState } from 'react';
import type { ReactNode } from 'react';
import {
  type AddressDetail,
  type AuxPowEvidence,
  type BlockDetail,
  type BlockSummary,
  type Envelope,
  type ParentObservation,
  type Reorg,
  type Status,
  type TransactionDetail,
  type TransactionSummary,
  ExplorerApiError,
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
import {
  AddressBalanceChart,
  MergeMiningAnalytics,
  NetworkAnalytics,
  WcashValuePools,
} from './explorer-analytics';
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
          return;
        }
        setResource({
          envelope: null,
          error: 'The indexed block feed is unavailable.',
          loading: false,
        });
      },
    );
    return () => controller.abort();
  }, [cursor]);

  return (
    <PageFrame
      eyebrow="Canonical chain"
      title="Blocks"
      description="Canonical Wcash blocks indexed from the node."
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
      eyebrow="Canonical chain"
      title="Transactions"
      description="Public transaction structure from canonical Wcash blocks."
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
      (error) =>
        setResource({
          envelope: null,
          error: blockLoadError(error, id),
          loading: false,
        }),
    );
    return () => controller.abort();
  }, [id]);

  const block = resource.envelope?.data;
  return (
    <PageFrame
      eyebrow="Blocks"
      title={block ? `#${block.height.toLocaleString()}` : 'Block'}
      description={
        block ? middleEllipsis(block.hash, 42) : 'Canonical block record'
      }
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
      (error) =>
        setResource({
          envelope: null,
          error: transactionLoadError(error),
          loading: false,
        }),
    );
    return () => controller.abort();
  }, [blockHash, txid]);

  return (
    <PageFrame
      eyebrow="Transactions"
      title="Transaction"
      description={middleEllipsis(txid, 42)}
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
      (error) =>
        setResource({
          envelope: null,
          error:
            error instanceof ExplorerApiError && error.status === 400
              ? 'That is not a valid Wcash transparent address for this network.'
              : error instanceof ExplorerApiError && error.status === 404
                ? 'That transparent address was not found on the canonical chain.'
                : 'Transparent-address data is temporarily unavailable.',
          loading: false,
        }),
    );
    return () => controller.abort();
  }, [address]);

  return (
    <PageFrame
      eyebrow="Transparent addresses"
      title="Address"
      description={middleEllipsis(address, 46)}
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
          return;
        }
        setResource({
          envelope: null,
          error: 'Merge-mining evidence is unavailable.',
          loading: false,
        });
      },
    );
    return () => controller.abort();
  }, [cursor]);

  const blocks = resource.envelope?.data ?? [];

  return (
    <PageFrame
      eyebrow="Auxiliary proof of work"
      title="Merge mining"
      description="Wcash AuxPoW validation and observations from configured Zcash nodes."
    >
      <MergeMiningAnalytics />
      {previewEnabled && resource.envelope === previewDashboard.blocks ? (
        <PreviewNotice />
      ) : null}
      <section className="panel overflow-hidden">
        <SectionHeader title="Verification states" />
        <div className="definition-grid">
          <Definition
            title="AuxPoW valid"
            body="The Equihash solution, child commitment, Merkle paths, indices, and Wcash target pass the pinned verifier."
          />
          <Definition
            title="Witness on best chain"
            body="The Wcash node recognizes this exact serialized proof witness on its canonical chain."
          />
          <Definition
            title="Zcash parent observed"
            body="Configured Zcash nodes return the parent block and its raw header matches the proof."
          />
        </div>
      </section>
      <section>
        <SectionTitle>Recent canonical evidence</SectionTitle>
        <ResourceState loading={resource.loading} error={resource.error}>
          <BlockTable blocks={blocks} loading={false} />
          <Pagination
            canGoBack={Boolean(cursor)}
            canGoForward={Boolean(resource.envelope?.meta.nextCursor)}
            onBack={() => setCursor(undefined)}
            onForward={() =>
              setCursor(resource.envelope?.meta.nextCursor ?? undefined)
            }
          />
        </ResourceState>
      </section>
    </PageFrame>
  );
}

export function NetworkPage() {
  const [status, setStatus] = useState<Resource<Status>>(emptyResource);
  const [reorgs, setReorgs] = useState<Resource<Reorg[]>>(emptyResource);

  useEffect(() => {
    const controller = new AbortController();
    void loadStatus(controller.signal).then(
      (envelope) => setStatus({ envelope, error: '', loading: false }),
      () => {
        setStatus({
          envelope: null,
          error: 'Network telemetry is unavailable.',
          loading: false,
        });
      },
    );
    void loadReorgs(controller.signal).then(
      (envelope) => setReorgs({ envelope, error: '', loading: false }),
      () =>
        setReorgs({
          envelope: null,
          error: 'Reorganization history is unavailable.',
          loading: false,
        }),
    );
    return () => controller.abort();
  }, []);

  const network = status.envelope?.data;
  return (
    <PageFrame
      eyebrow="Chain state"
      title="Network"
      description="Indexer status, consensus parameters, supply, and reorganization history."
    >
      <ResourceState loading={status.loading} error={status.error}>
        {network ? (
          <>
            <div className="stats-strip">
              <SummaryMetric
                label="Explorer height / node height"
                value={`${network.indexedHeight ?? '—'} / ${network.nodeHeight ?? '—'}`}
              />
              <SummaryMetric
                label="Target block time"
                value={`${network.targetSpacingSeconds}s`}
              />
              <SummaryMetric
                label="Initial reward"
                value={`${trimAmount(network.initialSubsidy.decimal)} ${network.symbol}`}
              />
              <SummaryMetric
                label="Next halving"
                value={`#${network.nextHalvingHeight.toLocaleString()}`}
              />
            </div>
            <section className="panel overflow-hidden">
              <SectionHeader title="Supply policy" />
              <dl className="record-grid p-4 sm:p-5">
                <Record
                  label="Maximum supply"
                  value={`${Number(network.maxSupply.decimal).toLocaleString()} ${network.symbol}`}
                  mono
                />
                <Record label="Decimal places" value="8" mono />
                <Record
                  label="Coinbase maturity"
                  value={`${network.coinbaseMaturity} blocks`}
                  mono
                />
                <Record
                  label="Recent average block time"
                  value={
                    network.observedSpacingSeconds
                      ? `${Math.round(network.observedSpacingSeconds)} seconds`
                      : 'Not enough data'
                  }
                  mono
                />
                <Record
                  label="Issued"
                  value={`${trimAmount(network.totalIssued.decimal)} ${network.symbol}`}
                  mono
                />
                <Record
                  label="Difficulty"
                  value={network.difficulty ?? 'Unavailable'}
                  mono
                />
              </dl>
            </section>
          </>
        ) : null}
      </ResourceState>
      <NetworkAnalytics />
      <section>
        <SectionTitle>Reorganizations</SectionTitle>
        <ResourceState loading={reorgs.loading} error={reorgs.error}>
          <ReorgTable events={reorgs.envelope?.data ?? []} />
        </ResourceState>
      </section>
    </PageFrame>
  );
}

function BlockDetailView({ block }: { block: BlockDetail }) {
  const auxpow = block.auxpow;
  const localProofValid = auxpow?.localValidationState === 'auxpow_verified';
  const localProofFailed = auxpow
    ? isFailureState(auxpow.localValidationState)
    : false;

  return (
    <div className="space-y-6">
      <div className="flex flex-wrap items-center gap-2">
        <Link href="/blocks" className="button-secondary">
          <ArrowLeft size={13} aria-hidden="true" /> Blocks
        </Link>
        {block.previousBlockHash ? (
          <Link
            href={`/block/${block.previousBlockHash}`}
            className="button-secondary"
          >
            <ArrowLeft size={13} aria-hidden="true" /> Previous
          </Link>
        ) : null}
        {block.nextBlockHash ? (
          <Link
            href={`/block/${block.nextBlockHash}`}
            className="button-secondary"
          >
            Next <ArrowRight size={13} aria-hidden="true" />
          </Link>
        ) : null}
        <a
          href={`/api/v1/blocks/${block.hash}/raw`}
          target="_blank"
          rel="noreferrer"
          className="button-secondary"
        >
          Raw witness <ExternalLink size={13} aria-hidden="true" />
        </a>
        <span className="status-label status-label-success ml-auto">
          <span className="status-dot" aria-hidden="true" />
          Canonical · {block.confirmations} confirmations
        </span>
      </div>

      <section className="panel overflow-hidden">
        <SectionHeader title="Overview" />
        <dl className="record-grid p-4 sm:p-5">
          <Record label="Block hash" value={block.hash} mono wide />
          <Record label="Witness hash" value={block.witnessHash} mono wide />
          <Record label="Mined at" value={relativeTime(block.time)} />
          <Record
            label="Size"
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
            label="Block reward"
            value={`${trimAmount(block.reward.decimal)} ${block.reward.symbol}`}
            mono
          />
          <Record label="Nonce" value={block.nonce} mono wide />
          <Record label="Merkle root" value={block.merkleRoot} mono wide />
          <Record
            label="Block commitments"
            value={block.blockCommitments ?? 'Not present'}
            mono
            wide
          />
        </dl>
      </section>

      <section id="auxpow" className="panel scroll-mt-24 overflow-hidden">
        <div className="section-heading-row">
          <h2>AuxPoW</h2>
          {auxpow ? (
            <span
              className={`status-label ${
                localProofValid
                  ? 'status-label-success'
                  : localProofFailed
                    ? 'status-label-danger'
                    : 'status-label-info'
              }`}
            >
              <span className="status-dot" aria-hidden="true" />
              {localProofValid
                ? 'AuxPoW valid'
                : humanize(auxpow.localValidationState)}
            </span>
          ) : (
            <span className="status-label status-label-info">
              <span className="status-dot" aria-hidden="true" /> Genesis work
            </span>
          )}
        </div>
        {auxpow ? (
          <AuxPowDetail auxpow={auxpow} />
        ) : (
          <p className="p-5 text-sm text-[var(--muted)]">
            Genesis has no auxiliary parent proof.
          </p>
        )}
      </section>

      <section>
        <SectionTitle>Transactions</SectionTitle>
        <TransactionTable transactions={block.transactions} />
      </section>

      {block.valuePools.length ? (
        <section>
          <SectionTitle>Value pools</SectionTitle>
          <WcashValuePools pools={block.valuePools} />
        </section>
      ) : null}
    </div>
  );
}

function AuxPowDetail({ auxpow }: { auxpow: AuxPowEvidence }) {
  return (
    <div>
      <div className="proof-grid">
        <ProofStage number="01" title="Wcash witness">
          <CompactRecord
            label="State"
            value={humanize(auxpow.exactWitnessState)}
          />
          <CompactRecord
            label="Confirmations"
            value={
              auxpow.witnessConfirmations?.toLocaleString() ?? 'Unavailable'
            }
          />
          <CompactRecord label="Witness hash" value={auxpow.witnessHash} mono />
        </ProofStage>
        <ProofStage number="02" title="Local validation">
          <CompactRecord
            label="State"
            value={humanize(auxpow.localValidationState)}
          />
          <CompactRecord
            label="Format"
            value={`WCAZ v${auxpow.proofVersion} · ${auxpow.proofSize.toLocaleString()} bytes`}
            mono
          />
          <CompactRecord
            label="Verifier"
            value={auxpow.verifierVersion ?? 'Pinned verifier'}
            mono
          />
          <CompactRecord
            label="Parent hash meets target"
            value={auxpow.parentHashMeetsClaimedTarget ? 'Met' : 'Not met'}
          />
          <CompactRecord
            label="Checked at"
            value={relativeTime(auxpow.verifiedAt)}
          />
        </ProofStage>
        <ProofStage number="03" title="Zcash lookup">
          <CompactRecord
            label="State"
            value={humanize(auxpow.parentLookupState)}
          />
          <CompactRecord
            label="Source states agree"
            value={
              auxpow.parentSourcesAgree
                ? 'Yes'
                : auxpow.parentLookupState === 'disagreement'
                  ? 'No — disagreement'
                  : 'Agreement not established'
            }
          />
          <CompactRecord
            label="Header bits"
            value={auxpow.parentHeaderBits}
            mono
          />
          <CompactRecord
            label="Parent block"
            value={auxpow.parentBlockHash}
            mono
          />
        </ProofStage>
      </div>

      <div className="border-t border-[var(--border)] p-4 sm:p-5">
        <dl className="record-grid">
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
          <Record
            label="Parent Merkle path"
            value={`depth ${auxpow.parentMerkleDepth}, index ${auxpow.parentCoinbaseIndex}`}
            mono
          />
          <Record
            label="Auth-data Merkle path"
            value={`depth ${auxpow.authDataMerkleDepth}, index ${auxpow.authDataCoinbaseIndex}`}
            mono
          />
          <Record
            label="Auxiliary Merkle path"
            value={`depth ${auxpow.auxiliaryMerkleDepth}, index ${auxpow.auxiliaryIndex}`}
            mono
          />
          <Record label="Verifier note" value={auxpow.meaning} />
        </dl>
        <div className="mt-5 flex flex-wrap gap-2">
          <ExternalEvidence
            href={auxpow.parentBlockUrl}
            label="Open Zcash block"
          />
          <ExternalEvidence
            href={auxpow.parentCoinbaseTxUrl}
            label="Open coinbase transaction"
          />
        </div>
      </div>

      <div className="border-t border-[var(--border)]">
        <div className="section-heading-row">
          <h3>Zcash source checks</h3>
        </div>
        <ParentObservationTable observations={auxpow.observations} />
      </div>
    </div>
  );
}

function ParentObservationTable({
  observations,
}: {
  observations: ParentObservation[];
}) {
  if (!observations.length) {
    return (
      <p className="p-5 text-sm text-[var(--muted)]">No parent observations.</p>
    );
  }

  return (
    <div className="overflow-x-auto">
      <table className="data-table">
        <caption className="sr-only">Zcash parent-node observations</caption>
        <thead>
          <tr>
            <th>Source</th>
            <th>State</th>
            <th>Parent</th>
            <th>Embedded header</th>
            <th>Work</th>
            <th>Checked (UTC)</th>
          </tr>
        </thead>
        <tbody>
          {observations.map((observation) => (
            <tr key={`${observation.sourceName}:${observation.checkedAt}`}>
              <td data-label="Source" className="mono">
                {observation.sourceName}
              </td>
              <td data-label="State">
                <span
                  className={`status-label ${parentObservationClass(observation)}`}
                >
                  <span className="status-dot" aria-hidden="true" />
                  {humanize(observation.observationState)}
                </span>
              </td>
              <td data-label="Parent" className="mono">
                {observation.parentHeight === null
                  ? 'Height unavailable'
                  : `#${observation.parentHeight.toLocaleString()}`}
                <span className="mt-1 block text-[var(--faint)]">
                  {observation.parentConfirmations === null
                    ? 'Confirmations unavailable'
                    : `${observation.parentConfirmations.toLocaleString()} confirmations`}
                </span>
              </td>
              <td data-label="Embedded header">
                {observation.embeddedHeaderMatches === null
                  ? 'Not checked'
                  : observation.embeddedHeaderMatches
                    ? 'Matches'
                    : 'Mismatch'}
              </td>
              <td data-label="Work" className="mono">
                <span>{observation.parentBits ?? 'Bits unavailable'}</span>
                <span className="mt-1 block text-[var(--faint)]">
                  {observation.parentDifficultyText
                    ? `Difficulty ${observation.parentDifficultyText}`
                    : 'Difficulty unavailable'}
                </span>
              </td>
              <td data-label="Checked (UTC)" className="whitespace-nowrap">
                <span>{relativeTime(observation.checkedAt)}</span>
                {observation.parentTime ? (
                  <span className="mt-1 block text-[var(--faint)]">
                    Parent {relativeTime(observation.parentTime)}
                  </span>
                ) : null}
              </td>
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}

function TransactionDetailView({
  transaction,
}: {
  transaction: TransactionDetail;
}) {
  const legacyShieldedActions = countLegacyShieldedActions(transaction);

  return (
    <div className="space-y-6">
      <section className="panel overflow-hidden">
        <div className="section-heading-row">
          <h2>Overview</h2>
          <Link
            href={`/block/${transaction.blockHeight}`}
            className="text-xs font-semibold text-[var(--brand)]"
          >
            Block #{transaction.blockHeight.toLocaleString()}
          </Link>
        </div>
        <dl className="record-grid p-4 sm:p-5">
          <Record label="Transaction ID" value={transaction.txid} mono wide />
          <Record
            label="Authorization digest"
            value={
              transaction.authDigest ??
              'Not defined for this transaction version'
            }
            mono={transaction.authDigest !== null}
            wide
          />
          {transaction.instanceDigestKind === 'explorer-raw-hash' ? (
            <Record
              label="Explorer instance fingerprint"
              value={transaction.instanceDigest}
              mono
              wide
            />
          ) : null}
          <Record label="Kind" value={humanize(transaction.kind)} />
          <Record label="Version" value={transaction.version.toString()} mono />
          <Record
            label="Size"
            value={`${transaction.sizeBytes.toLocaleString()} bytes`}
            mono
          />
          <Record
            label="Position in block"
            value={transaction.position.toString()}
            mono
          />
          <Record
            label="Transparent output total"
            value={`${trimAmount(transaction.publicOutputValue.decimal)} ${transaction.publicOutputValue.symbol}`}
            mono
          />
          <Record
            label="Fee"
            value={formatAmount(
              transaction.fee,
              transaction.isCoinbase ? 'Not applicable' : 'Not reported',
            )}
            mono
          />
          <Record
            label="Value balance"
            value={formatAmount(transaction.valueBalance, 'Not present')}
            mono
          />
          <Record
            label="Transparent inputs / outputs"
            value={`${transaction.transparentInputCount} / ${transaction.transparentOutputCount}`}
            mono
          />
          <Record
            label="Ironwood actions"
            value={transaction.ironwoodActionCount.toString()}
            mono
          />
          <Record
            label="Block time"
            value={relativeTime(transaction.blockTime)}
          />
        </dl>
      </section>

      {transaction.ironwoodActionCount > 0 ? (
        <PrivacyNotice text={transaction.privacyNotice} />
      ) : null}

      {legacyShieldedActions > 0 ? (
        <UnexpectedLegacyShieldedNotice count={legacyShieldedActions} />
      ) : null}

      <section className="panel overflow-hidden">
        <SectionHeader title="Ironwood shielded structure" />
        <dl className="record-grid p-4 sm:p-5">
          <Record
            label="Ironwood actions"
            value={transaction.ironwoodActionCount.toString()}
            mono
          />
        </dl>
      </section>

      <section>
        <SectionTitle>Transparent inputs</SectionTitle>
        <div className="table-shell">
          <table className="data-table">
            <caption className="sr-only">
              Transparent transaction inputs
            </caption>
            <thead>
              <tr>
                <th>Index</th>
                <th>Source</th>
                <th>Sequence</th>
              </tr>
            </thead>
            <tbody>
              {transaction.inputs.length ? (
                transaction.inputs.map((input) => (
                  <tr key={input.inputIndex}>
                    <td data-label="Index" className="mono">
                      {input.inputIndex}
                    </td>
                    <td
                      data-label="Source"
                      data-wide="true"
                      className="mono break-all"
                    >
                      {input.coinbaseData
                        ? `Coinbase ${input.coinbaseData}`
                        : `${input.previousTxid}:${input.previousOutputIndex}`}
                    </td>
                    <td data-label="Sequence" className="mono">
                      {input.sequence?.toString() ?? '—'}
                    </td>
                  </tr>
                ))
              ) : (
                <tr>
                  <td colSpan={3}>No transparent inputs.</td>
                </tr>
              )}
            </tbody>
          </table>
        </div>
      </section>

      <section>
        <SectionTitle>Transparent outputs</SectionTitle>
        <div className="table-shell">
          <table className="data-table">
            <caption className="sr-only">
              Transparent transaction outputs
            </caption>
            <thead>
              <tr>
                <th>Index</th>
                <th>Address or script</th>
                <th>Type</th>
                <th>Value</th>
              </tr>
            </thead>
            <tbody>
              {transaction.outputs.length ? (
                transaction.outputs.map((output) => (
                  <tr key={output.index}>
                    <td data-label="Index" className="mono">
                      {output.index}
                    </td>
                    <td
                      data-label="Address or script"
                      data-wide="true"
                      className="mono break-all"
                    >
                      {output.address ??
                        output.scriptHex ??
                        'Non-address script'}
                    </td>
                    <td data-label="Type">{output.scriptType ?? 'Unknown'}</td>
                    <td data-label="Value" className="mono whitespace-nowrap">
                      {trimAmount(output.value.decimal)} {output.value.symbol}
                    </td>
                  </tr>
                ))
              ) : (
                <tr>
                  <td colSpan={4}>No transparent outputs.</td>
                </tr>
              )}
            </tbody>
          </table>
        </div>
      </section>
    </div>
  );
}

function AddressDetailView({ address }: { address: AddressDetail }) {
  const activityTruncated = address.transactionCount > address.activity.length;

  return (
    <div className="space-y-6">
      <PrivacyNotice text={address.scopeNotice} />
      {address.canonicalDoubleSpendAnomalies > 0 ? (
        <div
          role="alert"
          className="panel flex items-start gap-3 border-[var(--danger)] p-4 text-sm"
        >
          <AlertTriangle
            className="mt-0.5 shrink-0 text-[var(--danger)]"
            size={16}
          />
          <p>
            The index detected{' '}
            {address.canonicalDoubleSpendAnomalies.toLocaleString()} canonical
            double-spend anomaly record(s). Do not rely on this balance until
            the index is audited.
          </p>
        </div>
      ) : null}
      <section className="panel overflow-hidden">
        <SectionHeader title="Overview" />
        <dl className="record-grid p-4 sm:p-5">
          <Record label="Address" value={address.address} mono wide />
          <Record label="Type" value={humanize(address.addressType)} />
          <Record
            label="UTXOs"
            value={address.utxoCount.toLocaleString()}
            mono
          />
          <Record
            label="Transactions"
            value={address.transactionCount.toLocaleString()}
            mono
          />
          <Record
            label="Mined transactions / outputs"
            value={`${address.minedTransactionCount.toLocaleString()} / ${address.minedOutputCount.toLocaleString()}`}
            mono
          />
          <Record
            label="First activity"
            value={formatActivityBoundary(
              address.firstSeenHeight,
              address.firstSeenAt,
            )}
            mono
          />
          <Record
            label="Last activity"
            value={formatActivityBoundary(
              address.lastSeenHeight,
              address.lastSeenAt,
            )}
            mono
          />
          <Record
            label="Coinbase maturity"
            value={`${address.coinbaseMaturity.toLocaleString()} blocks`}
            mono
          />
        </dl>
      </section>
      <div className="stats-strip">
        <SummaryMetric
          label="Transparent balance"
          value={formatAmount(address.unspent)}
        />
        <SummaryMetric
          label="Transparent received"
          value={formatAmount(address.totalReceived)}
        />
        <SummaryMetric
          label="Transparent sent"
          value={formatAmount(address.totalSent)}
        />
        <SummaryMetric
          label="Transactions"
          value={address.transactionCount.toLocaleString()}
        />
      </div>
      <div className="stats-strip stats-strip-three">
        <SummaryMetric
          label="Immature coinbase"
          value={formatAmount(address.immatureCoinbase)}
        />
        <SummaryMetric
          label="Mature coinbase requiring shielding"
          value={formatAmount(address.matureCoinbaseMustShield)}
        />
        <SummaryMetric
          label="Non-coinbase unspent"
          value={formatAmount(address.nonCoinbaseUnspent)}
        />
      </div>
      <AddressBalanceChart address={address} />
      <section>
        <SectionTitle>Transparent activity</SectionTitle>
        {activityTruncated ? (
          <p className="mb-3 text-xs leading-5 text-[var(--muted)]">
            Showing the latest {address.activity.length.toLocaleString()} of{' '}
            {address.transactionCount.toLocaleString()} canonical transparent
            transactions for this address.
          </p>
        ) : null}
        <div className="table-shell">
          <table className="data-table">
            <caption className="sr-only">
              Canonical transparent transactions involving this address
            </caption>
            <thead>
              <tr>
                <th>Block</th>
                <th>Transaction</th>
                <th>Direction</th>
                <th>Received</th>
                <th>Sent</th>
                <th>Net</th>
                <th>Balance after</th>
                <th>Time (UTC)</th>
              </tr>
            </thead>
            <tbody>
              {address.activity.length ? (
                address.activity.map((activity) => (
                  <tr
                    key={`${activity.txid}:${activity.instanceDigest}:${activity.blockHash}:${activity.position}`}
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
                        {middleEllipsis(activity.txid, 28)}
                      </Link>
                      {activity.authDigest ? (
                        <span className="sr-only">
                          {' '}
                          Authorization digest {activity.authDigest}
                        </span>
                      ) : null}
                    </td>
                    <td data-label="Direction">
                      {activity.isCoinbase
                        ? 'Coinbase in'
                        : humanize(activity.direction)}
                    </td>
                    <td
                      data-label="Received"
                      className="mono whitespace-nowrap"
                    >
                      {formatAmount(activity.received)}
                    </td>
                    <td data-label="Sent" className="mono whitespace-nowrap">
                      {formatAmount(activity.sent)}
                    </td>
                    <td data-label="Net" className="mono whitespace-nowrap">
                      {formatAmount(activity.net)}
                    </td>
                    <td
                      data-label="Balance after"
                      className="mono whitespace-nowrap"
                    >
                      {formatAmount(activity.balanceAfter)}
                    </td>
                    <td data-label="Time (UTC)" className="whitespace-nowrap">
                      <time dateTime={activity.blockTime}>
                        {relativeTime(activity.blockTime)}
                      </time>
                    </td>
                  </tr>
                ))
              ) : (
                <tr>
                  <td colSpan={8}>No transparent activity.</td>
                </tr>
              )}
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
  if (!transactions.length) {
    return (
      <div className="panel p-5 text-sm text-[var(--muted)]">
        No transactions found.
      </div>
    );
  }

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
            <th>Transparent output</th>
            <th>Ironwood actions</th>
            <th>Time (UTC)</th>
          </tr>
        </thead>
        <tbody>
          {transactions.map((transaction) => (
            <tr key={`${transaction.txid}:${transaction.instanceDigest}`}>
              <td data-label="Transaction ID" data-wide="true">
                <Link
                  href={`/tx/${transaction.txid}?block=${transaction.blockHash}`}
                  className="hash block"
                >
                  {middleEllipsis(transaction.txid, 28)}
                </Link>
                {transaction.authDigest ? (
                  <span className="sr-only">
                    {' '}
                    Authorization digest {transaction.authDigest}
                  </span>
                ) : null}
              </td>
              <td data-label="Kind">{humanize(transaction.kind)}</td>
              <td data-label="Block">
                <Link
                  href={`/block/${transaction.blockHeight}`}
                  className="mono text-[var(--brand)]"
                >
                  #{transaction.blockHeight.toLocaleString()}
                </Link>
              </td>
              <td
                data-label="Transparent output"
                className="mono whitespace-nowrap"
              >
                {formatAmount(transaction.publicOutputValue)}
              </td>
              <td data-label="Ironwood actions" className="mono">
                {transaction.ironwoodActionCount}
                {countLegacyShieldedActions(transaction) > 0 ? (
                  <span className="status-label status-label-danger ml-2">
                    Legacy fields
                  </span>
                ) : null}
              </td>
              <td data-label="Time (UTC)" className="whitespace-nowrap">
                <time dateTime={transaction.blockTime}>
                  {relativeTime(transaction.blockTime)}
                </time>
              </td>
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}

function ReorgTable({ events }: { events: Reorg[] }) {
  if (!events.length) {
    return (
      <div className="panel p-5 text-sm text-[var(--muted)]">
        No reorganizations recorded.
      </div>
    );
  }

  return (
    <div className="table-shell">
      <table className="data-table">
        <caption className="sr-only">Canonical chain reorganizations</caption>
        <thead>
          <tr>
            <th>Old tip</th>
            <th>Common ancestor</th>
            <th>Detected (UTC)</th>
            <th>Completed (UTC)</th>
          </tr>
        </thead>
        <tbody>
          {events.map((event) => (
            <tr key={event.eventId}>
              <td data-label="Old tip" data-wide="true">
                <span className="mono">
                  #{event.oldTipHeight.toLocaleString()}
                </span>
                <span className="hash mt-1 block max-w-xs text-[var(--faint)]">
                  {middleEllipsis(event.oldTipHash, 28)}
                </span>
              </td>
              <td data-label="Common ancestor" data-wide="true">
                <span className="mono">
                  #{event.commonAncestorHeight ?? '—'}
                </span>
                {event.commonAncestorHash ? (
                  <span className="hash mt-1 block max-w-xs text-[var(--faint)]">
                    {middleEllipsis(event.commonAncestorHash, 28)}
                  </span>
                ) : null}
              </td>
              <td data-label="Detected (UTC)" className="whitespace-nowrap">
                {relativeTime(event.detectedAt)}
              </td>
              <td data-label="Completed (UTC)" className="whitespace-nowrap">
                {event.completedAt
                  ? relativeTime(event.completedAt)
                  : 'In progress'}
              </td>
            </tr>
          ))}
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
  children: ReactNode;
}) {
  return (
    <div className="space-y-6">
      <header className="page-intro">
        <div>
          <div className="eyebrow">{eyebrow}</div>
          <h1 className="page-title">{title}</h1>
          <p className="page-description">{description}</p>
        </div>
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
  children: ReactNode;
}) {
  if (loading) {
    return (
      <output
        className="skeleton block h-64"
        aria-label="Loading indexed chain data"
      >
        <span className="sr-only">Loading indexed chain data</span>
      </output>
    );
  }
  if (error) {
    return (
      <div
        role="alert"
        className="panel flex items-start gap-3 border-[var(--danger)] p-5"
      >
        <AlertTriangle
          className="mt-0.5 shrink-0 text-[var(--danger)]"
          size={18}
        />
        <div>
          <div className="font-semibold">Explorer data unavailable</div>
          <p className="mt-1 text-sm leading-6 text-[var(--muted)]">{error}</p>
        </div>
      </div>
    );
  }
  return <>{children}</>;
}

function PreviewNotice() {
  return (
    <div className="panel flex items-start gap-3 border-[var(--warning)] p-4 text-sm">
      <AlertTriangle
        className="mt-0.5 shrink-0 text-[var(--warning)]"
        size={16}
      />
      <p>Showing a fixed local snapshot.</p>
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
    <nav aria-label="Pagination" className="mt-4 flex justify-end gap-2">
      <button
        type="button"
        disabled={!canGoBack}
        onClick={onBack}
        className="button-secondary disabled:opacity-40"
      >
        <ArrowLeft size={13} aria-hidden="true" /> Newest
      </button>
      <button
        type="button"
        disabled={!canGoForward}
        onClick={onForward}
        className="button-secondary disabled:opacity-40"
      >
        Older <ArrowRight size={13} aria-hidden="true" />
      </button>
    </nav>
  );
}

function SummaryMetric({ label, value }: { label: string; value: string }) {
  return (
    <div className="stat-cell">
      <div className="stat-label">{label}</div>
      <div className="stat-value">{value}</div>
    </div>
  );
}

function SectionHeader({ title }: { title: string }) {
  return (
    <div className="section-heading-row">
      <h2>{title}</h2>
    </div>
  );
}

function SectionTitle({ children }: { children: ReactNode }) {
  return <h2 className="mb-3 text-base font-semibold">{children}</h2>;
}

function Definition({ title, body }: { title: string; body: string }) {
  return (
    <div className="definition-cell">
      <h3 className="text-sm font-semibold">{title}</h3>
      <p className="mt-2 text-xs leading-5 text-[var(--muted)]">{body}</p>
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
      <dt className="stat-label">{label}</dt>
      <dd
        className={`${mono ? 'mono break-all' : ''} mt-1.5 text-sm leading-6`}
      >
        {value}
      </dd>
    </div>
  );
}

function ProofStage({
  number,
  title,
  children,
}: {
  number: string;
  title: string;
  children: ReactNode;
}) {
  return (
    <section className="proof-stage">
      <div className="flex items-baseline gap-2">
        <span className="mono text-[0.65rem] text-[var(--faint)]">
          {number}
        </span>
        <h3 className="text-sm font-semibold">{title}</h3>
      </div>
      <dl className="mt-4 space-y-3">{children}</dl>
    </section>
  );
}

function CompactRecord({
  label,
  value,
  mono = false,
}: {
  label: string;
  value: string;
  mono?: boolean;
}) {
  return (
    <div className="min-w-0">
      <dt className="text-[0.67rem] text-[var(--faint)]">{label}</dt>
      <dd className={`${mono ? 'mono break-all' : ''} mt-1 text-xs leading-5`}>
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
      className="button-secondary"
    >
      {label} <ExternalLink size={13} aria-hidden="true" />
    </a>
  );
}

function PrivacyNotice({ text }: { text: string }) {
  return (
    <aside className="border-l-2 border-[var(--brand)] py-1 pl-4">
      <div className="text-sm font-semibold">Shielded data</div>
      <p className="mt-1 max-w-4xl text-sm leading-6 text-[var(--muted)]">
        {text}
      </p>
    </aside>
  );
}

function UnexpectedLegacyShieldedNotice({ count }: { count: number }) {
  return (
    <aside
      role="alert"
      className="panel flex items-start gap-3 border-[var(--danger)] p-4 text-sm"
    >
      <AlertTriangle
        className="mt-0.5 shrink-0 text-[var(--danger)]"
        size={16}
        aria-hidden="true"
      />
      <div>
        <strong>Forbidden legacy shielded fields detected</strong>
        <p className="mt-1 leading-6 text-[var(--muted)]">
          This transaction contains {count.toLocaleString()} Sapling or Orchard
          field entries. Wcash expects Ironwood as its only shielded protocol;
          treat this indexed record as a consensus anomaly.
        </p>
      </div>
    </aside>
  );
}

function countLegacyShieldedActions(transaction: TransactionSummary) {
  return (
    transaction.saplingSpendCount +
    transaction.saplingOutputCount +
    transaction.orchardActionCount
  );
}

function blockLoadError(error: unknown, id: string) {
  if (error instanceof ExplorerApiError) {
    if (error.status === 400) {
      return 'That is not a valid Wcash block height or hash.';
    }
    if (error.status === 404) {
      return `Block ${id} was not found on the indexed canonical chain.`;
    }
  }
  return 'Block data is temporarily unavailable.';
}

function transactionLoadError(error: unknown) {
  if (error instanceof ExplorerApiError) {
    if (error.status === 400) {
      return 'That is not a valid Wcash transaction identifier.';
    }
    if (error.status === 404) {
      return 'That transaction was not found on the indexed canonical chain.';
    }
  }
  return 'Transaction data is temporarily unavailable.';
}

function parentObservationClass(observation: ParentObservation) {
  const state = observation.observationState.toLowerCase();
  if (
    observation.embeddedHeaderMatches === false ||
    ['disagreement', 'invalid', 'rejected', 'error'].some((failure) =>
      state.includes(failure),
    )
  ) {
    return 'status-label-danger';
  }
  if (state === 'canonical') return 'status-label-success';
  if (state.includes('orphan')) return 'status-label-warning';
  return 'status-label-info';
}

function isFailureState(value: string) {
  const state = value.toLowerCase();
  return ['error', 'failed', 'invalid', 'rejected', 'unavailable'].some(
    (failure) => state.includes(failure),
  );
}

function formatAmount(
  amount: { decimal: string; symbol: string } | null,
  fallback = 'Unavailable',
) {
  return amount ? `${trimAmount(amount.decimal)} ${amount.symbol}` : fallback;
}

function formatActivityBoundary(height: number | null, time: string | null) {
  if (height === null) return 'Unavailable';
  return time
    ? `#${height.toLocaleString()} · ${relativeTime(time)}`
    : `#${height.toLocaleString()}`;
}
