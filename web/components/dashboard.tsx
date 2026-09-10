'use client';

import Link from 'next/link';
import {
  ArrowUpRight,
  Boxes,
  Check,
  CircleDollarSign,
  Clock3,
  DatabaseZap,
  GitCommitHorizontal,
  GitMerge,
  LockKeyhole,
  Pickaxe,
  RefreshCw,
  ShieldCheck,
} from 'lucide-react';
import { useEffect, useMemo, useState } from 'react';
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

  useEffect(() => {
    const controller = new AbortController();
    const refresh = () => {
      if (document.visibilityState === 'hidden') return;
      void loadDashboard(controller.signal).then(
        (result) => {
          setDashboard(result);
          setLoading(false);
        },
        () => setLoading(false),
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
  const latestProof = useMemo(
    () =>
      dashboard.proofBlock ??
      dashboard.blocks.data.find(
        (block) => block.mergeMining.parentLookupState === 'canonical',
      ) ??
      dashboard.blocks.data[0],
    [dashboard.blocks.data, dashboard.proofBlock],
  );

  return (
    <div className="space-y-8">
      <section className="grid items-end gap-8 lg:grid-cols-[minmax(0,1fr)_420px]">
        <div>
          <div className="eyebrow flex items-center gap-2">
            <ShieldCheck size={15} aria-hidden="true" /> Independent chain
            evidence
          </div>
          <h1 className="mt-4 max-w-4xl text-[clamp(2.35rem,6vw,5.6rem)] font-extrabold leading-[0.93] tracking-[-0.065em]">
            See the block.
            <br />
            <span className="text-[var(--brand)]">Verify the work.</span>
          </h1>
          <p className="mt-6 max-w-2xl text-base leading-7 text-[var(--muted)] sm:text-lg">
            WcashExplorer indexes Wcash separately from Zcash and shows exactly
            what the merge-mining proof establishes—without guessing at private
            activity.
          </p>
        </div>
        <div className="panel p-5">
          <div className="flex items-center justify-between gap-4">
            <div>
              <div className="eyebrow">Chain health</div>
              <div className="mt-2 text-lg font-bold">
                Indexer is {status.status}
              </div>
            </div>
            <span
              className={`pill ${status.status === 'ready' ? 'pill-success' : status.status === 'preview' ? 'pill-warning' : 'pill-danger'}`}
            >
              <span className="status-dot" aria-hidden="true" />{' '}
              {status.lagBlocks ?? '—'} block lag
            </span>
          </div>
          <div className="mt-5 grid grid-cols-2 gap-3 border-t border-[var(--border)] pt-5 text-sm">
            <div>
              <div className="text-[var(--faint)]">Indexed tip</div>
              <div className="mono mt-1 font-bold">
                #{status.indexedHeight ?? '—'}
              </div>
            </div>
            <div>
              <div className="text-[var(--faint)]">Data mode</div>
              <div className="mt-1 flex items-center gap-2 font-bold">
                <DatabaseZap
                  size={15}
                  className="text-[var(--mint)]"
                  aria-hidden="true"
                />
                {dashboard.source === 'live'
                  ? 'Live API'
                  : dashboard.source === 'preview'
                    ? 'Labeled preview'
                    : 'API unavailable'}
              </div>
            </div>
          </div>
        </div>
      </section>

      <SearchBox />

      <section aria-label="Network summary" className="metric-grid">
        <Metric
          icon={<Boxes size={17} />}
          label="Canonical tip"
          value={`#${status.indexedHeight ?? '—'}`}
          foot={`${dashboard.status.meta.freshnessSeconds ?? '—'}s data age`}
        />
        <Metric
          icon={<Pickaxe size={17} />}
          label="Difficulty"
          value={status.difficulty ?? '—'}
          foot={`${status.targetSpacingSeconds}s target block time`}
        />
        <Metric
          icon={<Clock3 size={17} />}
          label="Observed spacing"
          value={
            status.observedSpacingSeconds
              ? `${Math.round(status.observedSpacingSeconds)}s`
              : '—'
          }
          foot="Rolling canonical sample"
        />
        <Metric
          icon={<CircleDollarSign size={17} />}
          label="Total issued"
          value={
            dashboard.source === 'unavailable'
              ? '—'
              : `${trimAmount(status.totalIssued.decimal)} ${status.symbol}`
          }
          foot={`Hard cap ${Number(status.maxSupply.decimal).toLocaleString()} ${status.maxSupply.symbol}`}
        />
      </section>

      <section className="grid min-w-0 gap-6 xl:grid-cols-[minmax(0,1.7fr)_minmax(340px,0.8fr)]">
        <div className="min-w-0">
          <SectionHeading
            eyebrow="Canonical chain"
            title="Latest Wcash blocks"
            href="/blocks"
            action="All blocks"
          />
          <BlockTable
            blocks={dashboard.blocks.data.slice(0, 8)}
            loading={loading}
          />
        </div>
        <div className="min-w-0">
          <SectionHeading
            eyebrow="Dual-chain evidence"
            title="Latest parent match"
          />
          {latestProof ? (
            <ProofCard block={latestProof} />
          ) : (
            <div className="panel p-6 text-sm leading-6 text-[var(--muted)]">
              No verified merge-mining record is available while the index is
              offline.
            </div>
          )}
        </div>
      </section>

      <section className="panel grid gap-7 p-6 md:grid-cols-[1fr_1.1fr] md:p-8">
        <div>
          <div className="eyebrow flex items-center gap-2">
            <LockKeyhole size={15} aria-hidden="true" /> Privacy-honest by
            design
          </div>
          <h2 className="mt-3 text-2xl font-extrabold tracking-[-0.04em] sm:text-3xl">
            Hidden means hidden—not zero.
          </h2>
        </div>
        <p className="text-sm leading-7 text-[var(--muted)] sm:text-base">
          The explorer shows public transparent inputs and outputs, aggregate
          value-pool movement, nullifiers, commitments, and Ironwood action
          counts. It never invents shielded senders, recipients, note amounts,
          balances, or memos that the protocol keeps private.
        </p>
      </section>
    </div>
  );
}

function Metric({
  icon,
  label,
  value,
  foot,
}: {
  icon: React.ReactNode;
  label: string;
  value: string;
  foot: string;
}) {
  return (
    <article className="metric panel">
      <div className="metric-label">
        <span>{label}</span>
        <span className="text-[var(--brand)]" aria-hidden="true">
          {icon}
        </span>
      </div>
      <div className="metric-value">{value}</div>
      <div className="metric-foot">{foot}</div>
    </article>
  );
}

function SectionHeading({
  eyebrow,
  title,
  href,
  action,
}: {
  eyebrow: string;
  title: string;
  href?: string;
  action?: string;
}) {
  return (
    <div className="mb-4 flex min-h-12 items-end justify-between gap-4">
      <div>
        <div className="eyebrow">{eyebrow}</div>
        <h2 className="mt-1 text-xl font-extrabold tracking-[-0.035em]">
          {title}
        </h2>
      </div>
      {href && action ? (
        <Link
          href={href}
          className="flex min-h-11 items-center gap-1 text-sm font-bold text-[var(--brand)]"
        >
          {action} <ArrowUpRight size={15} aria-hidden="true" />
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
        className="skeleton block h-96"
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
            <th>Transactions</th>
            <th>Reward</th>
            <th>Merge evidence</th>
            <th>Time (UTC)</th>
          </tr>
        </thead>
        <tbody>
          {blocks.map((block) => (
            <tr key={`${block.hash}:${block.witnessHash}`}>
              <td data-label="Height">
                <Link
                  href={`/block/${block.height}`}
                  className="mono font-bold text-[var(--brand)]"
                >
                  #{block.height.toLocaleString()}
                </Link>
              </td>
              <td data-label="Block hash" data-wide="true">
                <Link
                  href={`/block/${block.hash}`}
                  className="hash block max-w-[240px]"
                  title={block.hash}
                >
                  {middleEllipsis(block.hash, 20)}
                </Link>
              </td>
              <td data-label="Transactions" className="mono">
                {block.transactionCount}
              </td>
              <td data-label="Reward" className="mono whitespace-nowrap">
                {trimAmount(block.reward.decimal)} {block.reward.symbol}
              </td>
              <td data-label="Merge evidence">
                {block.mergeMining.localValidationState !==
                'auxpow_verified' ? (
                  <span className="pill pill-danger">
                    <RefreshCw size={13} aria-hidden="true" /> Unverified
                  </span>
                ) : block.mergeMining.parentLookupState === 'canonical' &&
                  block.mergeMining.parentSourcesAgree ? (
                  <span className="pill pill-success">
                    <Check size={13} aria-hidden="true" /> Parent observed
                    canonical
                  </span>
                ) : (
                  <span className="pill pill-info">
                    <GitCommitHorizontal size={13} aria-hidden="true" /> AuxPoW
                    verified
                  </span>
                )}
              </td>
              <td
                data-label="Time (UTC)"
                className="whitespace-nowrap text-[var(--muted)]"
              >
                <time dateTime={block.time}>{relativeTime(block.time)}</time>
              </td>
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}

function ProofCard({ block }: { block: BlockSummary }) {
  const locallyValid =
    block.mergeMining.localValidationState === 'auxpow_verified';
  const exactWitness = block.mergeMining.exactWitnessState === 'best_chain';
  const parentCanonical =
    block.mergeMining.parentLookupState === 'canonical' &&
    block.mergeMining.parentSourcesAgree;
  return (
    <article className="panel overflow-hidden">
      <div className="border-b border-[var(--border)] bg-[var(--raised)] p-5">
        <div className="flex flex-wrap items-center justify-between gap-3">
          <span
            className={`pill ${locallyValid ? 'pill-success' : 'pill-danger'}`}
          >
            {locallyValid ? (
              <Check size={13} aria-hidden="true" />
            ) : (
              <RefreshCw size={13} aria-hidden="true" />
            )}
            {locallyValid ? 'AuxPoW verified' : 'Proof not verified'}
          </span>
          <span className="mono text-xs text-[var(--faint)]">
            Exact indexed witness
          </span>
        </div>
        <div className="mt-5 flex items-center gap-3">
          <div className="flex h-11 w-11 items-center justify-center rounded-xl bg-[var(--brand)] text-[var(--brand-ink)]">
            <GitMerge size={22} aria-hidden="true" />
          </div>
          <div>
            <div className="text-sm text-[var(--muted)]">Wcash block</div>
            <Link
              href={`/block/${block.height}`}
              className="mono text-lg font-bold"
            >
              #{block.height.toLocaleString()}
            </Link>
          </div>
        </div>
      </div>
      <div className="space-y-5 p-5">
        <EvidenceRow
          label="Exact Wcash witness"
          value={humanize(block.mergeMining.exactWitnessState)}
          state={exactWitness ? 'success' : 'info'}
        />
        <EvidenceRow
          label="Local consensus checks"
          value={locallyValid ? 'Passed' : 'Not verified'}
          state={locallyValid ? 'success' : 'info'}
        />
        <EvidenceRow
          label="Zcash Testnet parent"
          value={
            parentCanonical
              ? `Canonical #${block.mergeMining.parentHeight?.toLocaleString()}`
              : 'Not observed as a block'
          }
          state={parentCanonical ? 'success' : 'info'}
        />
        <div className="border-t border-[var(--border)] pt-4">
          <div className="mb-2 text-xs font-bold uppercase tracking-[0.08em] text-[var(--faint)]">
            Parent header hash
          </div>
          <div
            className="hash text-xs"
            title={block.mergeMining.parentBlockHash ?? undefined}
          >
            {block.mergeMining.parentBlockHash ?? 'No parent hash available'}
          </div>
        </div>
        <Link
          href={`/block/${block.height}#auxpow`}
          className="flex min-h-11 w-full items-center justify-center gap-2 rounded-[10px] border border-[var(--border-strong)] text-sm font-bold hover:bg-[var(--raised)]"
        >
          Inspect proof ladder <ArrowUpRight size={15} aria-hidden="true" />
        </Link>
      </div>
    </article>
  );
}

function EvidenceRow({
  label,
  value,
  state,
}: {
  label: string;
  value: string;
  state: 'success' | 'info';
}) {
  return (
    <div className="flex items-start justify-between gap-4 text-sm">
      <span className="text-[var(--muted)]">{label}</span>
      <span
        className={`flex items-center gap-2 text-right font-bold ${state === 'success' ? 'text-[var(--mint)]' : 'text-[var(--info)]'}`}
      >
        {state === 'success' ? (
          <Check size={15} aria-hidden="true" />
        ) : (
          <RefreshCw size={14} aria-hidden="true" />
        )}
        {value}
      </span>
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
