'use client';

import Link from 'next/link';
import { AlertTriangle } from 'lucide-react';
import { useEffect, useState } from 'react';
import {
  type BlockSummary,
  type DashboardData,
  loadDashboard,
  previewDashboard,
  previewEnabled,
  unavailableDashboard,
} from '@/lib/explorer-data';
import { SearchBox } from './search-box';

export function Dashboard() {
  const [dashboard, setDashboard] = useState<DashboardData>(
    previewEnabled ? previewDashboard : unavailableDashboard,
  );
  const [loading, setLoading] = useState(true);
  const [observedAt, setObservedAt] = useState<number | null>(null);

  useEffect(() => {
    const controller = new AbortController();
    const refresh = () => {
      if (document.visibilityState === 'hidden') return;
      void loadDashboard(controller.signal).then(
        (result) => {
          setDashboard(result);
          setLoading(false);
          setObservedAt(Date.now());
        },
        () => {
          setLoading(false);
          setObservedAt(Date.now());
        },
      );
    };
    refresh();
    const interval = window.setInterval(refresh, 15_000);
    document.addEventListener('visibilitychange', refresh);
    return () => {
      controller.abort();
      window.clearInterval(interval);
      document.removeEventListener('visibilitychange', refresh);
    };
  }, []);

  const status = dashboard.status.data;
  const latestProof =
    dashboard.source === 'live'
      ? dashboard.blocks.data[0]
      : (dashboard.proofBlock ?? dashboard.blocks.data[0]);
  const latestBlockAgeSeconds = secondsSince(
    status.latestBlockTime,
    observedAt,
  );
  const isReady = status.status === 'ready';
  const statusFailed = isFailureState(status.status);
  const isPreview = dashboard.source === 'preview';
  const staleAfterSeconds = Math.max(status.targetSpacingSeconds * 8, 15 * 60);
  const chainAppearsStale =
    dashboard.source === 'live' &&
    latestBlockAgeSeconds !== null &&
    latestBlockAgeSeconds > staleAfterSeconds;

  return (
    <div className="space-y-6">
      <section className="page-intro">
        <div>
          <div className="eyebrow">Wcash Testnet</div>
          <h1 className="page-title">Block explorer</h1>
          <p className="page-description">
            Search blocks, transactions, and transparent addresses.
          </p>
        </div>
        <div
          className={`chain-state ${
            isReady
              ? 'status-label-success'
              : statusFailed
                ? 'status-label-danger'
                : isPreview
                  ? 'status-label-warning'
                  : 'status-label-info'
          }`}
        >
          <span className="status-dot" aria-hidden="true" />
          <strong>
            {isReady
              ? 'Synchronized'
              : isPreview
                ? 'Preview data'
                : humanize(status.status)}
          </strong>
          {status.indexedHeight === null ? null : (
            <>
              <span>·</span>
              <span className="mono">#{status.indexedHeight}</span>
            </>
          )}
          {dashboard.status.meta.freshnessSeconds === null &&
          !isPreview ? null : (
            <>
              <span>·</span>
              <span>
                {isPreview
                  ? 'fixed snapshot'
                  : `index checked ${dashboard.status.meta.freshnessSeconds}s ago`}
              </span>
            </>
          )}
        </div>
      </section>

      {chainAppearsStale ? (
        <aside className="panel flex items-start gap-3 border-[var(--warning)] p-4 text-sm">
          <AlertTriangle
            className="mt-0.5 shrink-0 text-[var(--warning)]"
            size={17}
            aria-hidden="true"
          />
          <div>
            <strong>Block production appears paused</strong>
            <p className="mt-1 leading-6 text-[var(--muted)]">
              No new Wcash block has been indexed for{' '}
              {formatAge(latestBlockAgeSeconds)}. The explorer is synchronized
              with its configured node; that does not mean the network is
              currently producing blocks.
            </p>
          </div>
        </aside>
      ) : null}

      <SearchBox />

      <section aria-label="Network summary" className="stats-strip">
        <Metric
          label="Latest block"
          value={`#${status.indexedHeight ?? '—'}`}
          foot={
            status.latestBlockTime
              ? `Mined ${relativeTime(status.latestBlockTime)}`
              : status.lagBlocks === null
                ? 'Node comparison unavailable'
                : status.lagBlocks === 0
                  ? 'Synced with node'
                  : `${status.lagBlocks} ${status.lagBlocks === 1 ? 'block' : 'blocks'} behind`
          }
        />
        <Metric
          label="Difficulty"
          value={status.difficulty ?? '—'}
          foot={`${status.targetSpacingSeconds}s target`}
        />
        <Metric
          label="Avg. block time"
          value={
            status.observedSpacingSeconds
              ? `${Math.round(status.observedSpacingSeconds)}s`
              : '—'
          }
          foot="Recent canonical blocks"
        />
        <Metric
          label="Issued"
          value={
            dashboard.source === 'unavailable'
              ? '—'
              : `${trimAmount(status.totalIssued.decimal)} ${status.symbol}`
          }
          foot={`Cap ${Number(status.maxSupply.decimal).toLocaleString()} ${status.maxSupply.symbol}`}
        />
      </section>

      <section className="min-w-0">
        <SectionHeading
          title="Latest blocks"
          href="/blocks"
          action="View all"
        />
        <BlockTable
          blocks={dashboard.blocks.data.slice(0, 10)}
          loading={loading}
        />
      </section>

      <section className="min-w-0">
        <SectionHeading title="Latest AuxPoW check" />
        {latestProof ? (
          <ProofRecord block={latestProof} />
        ) : (
          <div className="panel p-5 text-sm text-[var(--muted)]">
            Merge-mining data is unavailable.
          </div>
        )}
      </section>
    </div>
  );
}

function Metric({
  label,
  value,
  foot,
}: {
  label: string;
  value: string;
  foot: string;
}) {
  return (
    <article className="stat-cell">
      <div className="stat-label">{label}</div>
      <div className="stat-value">{value}</div>
      <div className="stat-foot">{foot}</div>
    </article>
  );
}

function SectionHeading({
  title,
  href,
  action,
}: {
  title: string;
  href?: string;
  action?: string;
}) {
  return (
    <div className="section-heading">
      <h2>{title}</h2>
      {href && action ? (
        <Link
          href={href}
          className="text-xs font-semibold text-[var(--muted)] hover:text-[var(--text)]"
        >
          {action}
        </Link>
      ) : null}
    </div>
  );
}

export function BlockTable({
  blocks,
  loading,
}: {
  blocks: BlockSummary[];
  loading: boolean;
}) {
  if (loading && blocks.length === 0) {
    return (
      <output
        className="skeleton block h-80"
        aria-label="Loading latest blocks"
      >
        <span className="sr-only">Loading latest blocks</span>
      </output>
    );
  }

  return (
    <div className="table-shell">
      <table className="data-table">
        <caption className="sr-only">Canonical Wcash blocks</caption>
        <thead>
          <tr>
            <th>Height</th>
            <th>Block hash</th>
            <th>Txs</th>
            <th>Reward</th>
            <th>AuxPoW</th>
            <th>Time (UTC)</th>
          </tr>
        </thead>
        <tbody>
          {blocks.map((block) => {
            const proofState = block.mergeMining.localValidationState;
            const localValid = proofState === 'auxpow_verified';
            const localFailed = isFailureState(proofState);
            const parentState = block.mergeMining.parentLookupState;
            const parentObserved = parentState === 'canonical';
            const parentDisagrees = parentState === 'disagreement';
            return (
              <tr key={`${block.hash}:${block.witnessHash}`}>
                <td data-label="Height">
                  <Link
                    href={`/block/${block.height}`}
                    className="mono font-semibold text-[var(--brand)]"
                  >
                    #{block.height.toLocaleString()}
                  </Link>
                </td>
                <td data-label="Block hash" data-wide="true">
                  <Link
                    href={`/block/${block.hash}`}
                    className="hash block max-w-[300px]"
                    title={block.hash}
                  >
                    {middleEllipsis(block.hash, 26)}
                  </Link>
                </td>
                <td data-label="Txs" className="mono">
                  {block.transactionCount}
                </td>
                <td data-label="Reward" className="mono whitespace-nowrap">
                  {trimAmount(block.reward.decimal)} {block.reward.symbol}
                </td>
                <td data-label="AuxPoW">
                  <span
                    className={`status-label ${
                      localFailed
                        ? 'status-label-danger'
                        : parentDisagrees
                          ? 'status-label-warning'
                          : parentObserved && localValid
                            ? 'status-label-success'
                            : 'status-label-info'
                    }`}
                  >
                    <span className="status-dot" aria-hidden="true" />
                    {localValid
                      ? parentObserved
                        ? 'Valid · parent observed'
                        : parentDisagrees
                          ? 'Valid · sources disagree'
                          : parentState === 'not_found'
                            ? 'Valid · parent not observed'
                            : `Valid · ${parentState.replaceAll('_', ' ')}`
                      : humanize(proofState)}
                  </span>
                </td>
                <td
                  data-label="Time (UTC)"
                  className="whitespace-nowrap text-[var(--muted)]"
                >
                  <time dateTime={block.time}>{relativeTime(block.time)}</time>
                </td>
              </tr>
            );
          })}
        </tbody>
      </table>
    </div>
  );
}

function ProofRecord({ block }: { block: BlockSummary }) {
  const proofState = block.mergeMining.localValidationState;
  const locallyValid = proofState === 'auxpow_verified';
  const localFailed = isFailureState(proofState);
  const exactWitness = block.mergeMining.exactWitnessState === 'best_chain';
  const parentObserved = block.mergeMining.parentLookupState === 'canonical';

  return (
    <article className="panel overflow-hidden">
      <div className="flex flex-wrap items-center justify-between gap-3 border-b border-[var(--border)] px-4 py-3">
        <Link
          href={`/block/${block.height}#auxpow`}
          className="mono font-semibold text-[var(--brand)]"
        >
          Block #{block.height.toLocaleString()}
        </Link>
        <span
          className={`status-label ${
            locallyValid
              ? 'status-label-success'
              : localFailed
                ? 'status-label-danger'
                : 'status-label-info'
          }`}
        >
          <span className="status-dot" aria-hidden="true" />
          {locallyValid ? 'AuxPoW valid' : humanize(proofState)}
        </span>
      </div>
      <dl className="divide-y divide-[var(--border)] px-4 text-sm">
        <EvidenceRow
          label="Wcash witness"
          value={
            exactWitness
              ? 'Best chain'
              : humanize(block.mergeMining.exactWitnessState)
          }
        />
        <EvidenceRow
          label="Zcash observation"
          value={
            parentObserved
              ? `Parent #${block.mergeMining.parentHeight?.toLocaleString()}`
              : humanize(block.mergeMining.parentLookupState)
          }
        />
        <EvidenceRow
          label="Source agreement"
          value={
            block.mergeMining.parentSourcesAgree
              ? 'Established'
              : block.mergeMining.parentLookupState === 'disagreement'
                ? 'Disagreement'
                : 'Not established'
          }
        />
        <EvidenceRow
          label="Parent block"
          value={
            block.mergeMining.parentBlockHash
              ? middleEllipsis(block.mergeMining.parentBlockHash, 30)
              : 'Unavailable'
          }
          mono
        />
      </dl>
      <div className="border-t border-[var(--border)] px-4 py-3 text-right">
        <Link
          href={`/block/${block.height}#auxpow`}
          className="text-xs font-semibold text-[var(--muted)] hover:text-[var(--text)]"
        >
          View proof details
        </Link>
      </div>
    </article>
  );
}

function EvidenceRow({
  label,
  value,
  mono = false,
}: {
  label: string;
  value: string;
  mono?: boolean;
}) {
  return (
    <div className="flex items-center justify-between gap-5 py-3">
      <dt className="text-[var(--muted)]">{label}</dt>
      <dd className={`${mono ? 'mono' : ''} text-right font-medium`}>
        {value}
      </dd>
    </div>
  );
}

export function trimAmount(value: string) {
  return value.replace(/\.0+$/, '').replace(/(\.\d*?)0+$/, '$1');
}

export function middleEllipsis(value: string, visible: number) {
  if (value.length <= visible) return value;
  const side = Math.floor((visible - 1) / 2);
  return `${value.slice(0, side)}…${value.slice(-side)}`;
}

export function humanize(value: string) {
  return value
    .replaceAll('_', ' ')
    .replace(/^./, (letter) => letter.toUpperCase());
}

export function relativeTime(value: string) {
  return new Date(value)
    .toISOString()
    .replace('T', ' ')
    .replace('.000Z', ' UTC');
}

function isFailureState(value: string) {
  const normalized = value.toLowerCase();
  return ['error', 'failed', 'invalid', 'rejected', 'unavailable'].some(
    (state) => normalized.includes(state),
  );
}

function secondsSince(value: string | null, observedAt: number | null) {
  if (!value || observedAt === null) return null;
  const timestamp = Date.parse(value);
  if (!Number.isFinite(timestamp)) return null;
  return Math.max(0, Math.floor((observedAt - timestamp) / 1_000));
}

function formatAge(seconds: number | null) {
  if (seconds === null) return 'an unknown period';
  if (seconds < 60) return `${seconds} seconds`;
  if (seconds < 3_600) return `${Math.floor(seconds / 60)} minutes`;
  if (seconds < 86_400) return `${Math.floor(seconds / 3_600)} hours`;
  return `${Math.floor(seconds / 86_400)} days`;
}
