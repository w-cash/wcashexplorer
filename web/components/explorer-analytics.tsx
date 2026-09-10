'use client';

import Link from 'next/link';
import { AlertTriangle, CheckCircle2 } from 'lucide-react';
import { useEffect, useState } from 'react';
import {
  type AddressDetail,
  type Amount,
  type Envelope,
  type MergeMiningStats,
  type NetworkHistory,
  type TransparentAddressStats,
  type TransparentRichList,
  type ValuePool,
  type ValuePoolHistory,
  loadMergeMiningStats,
  loadNetworkHistory,
  loadTransparentAddressStats,
  loadTransparentRichList,
  loadValuePoolHistory,
} from '@/lib/explorer-data';
import { middleEllipsis, relativeTime, trimAmount } from './dashboard';
import { type ChartPoint, ExplorerLineChart } from './explorer-chart';
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

export function MergeMiningAnalytics() {
  const [resource, setResource] =
    useState<Resource<MergeMiningStats>>(emptyResource);

  useEffect(() => {
    const controller = new AbortController();
    void loadMergeMiningStats(controller.signal).then(
      (envelope) => setResource({ envelope, error: '', loading: false }),
      () =>
        setResource({
          envelope: null,
          error: 'Canonical AuxPoW totals are unavailable.',
          loading: false,
        }),
    );
    return () => controller.abort();
  }, []);

  const stats = resource.envelope?.data;
  return (
    <InlineResource resource={resource}>
      {stats ? (
        <div className="space-y-4">
          <div className="analytics-summary-grid">
            <Metric
              label="Fully verified"
              value={`${stats.fullyVerifiedBlocks} / ${stats.eligibleChildBlocks}`}
            />
            <Metric
              label="Best-chain witnesses"
              value={stats.bestChainWitnessBlocks.toLocaleString()}
            />
            <Metric
              label="Canonical Zcash parents"
              value={stats.canonicalParentBlocks.toLocaleString()}
            />
            <Metric
              label="Independent parent sources"
              value={stats.observationSourceCount.toLocaleString()}
            />
            <Metric
              label="Anomaly blocks"
              value={stats.anomalyBlocks.toLocaleString()}
            />
          </div>
          <div
            className={`verification-panel ${
              stats.anomalyBlocks === 0
                ? 'verification-panel-success'
                : 'verification-panel-warning'
            }`}
          >
            {stats.anomalyBlocks === 0 ? (
              <CheckCircle2 size={18} aria-hidden="true" />
            ) : (
              <AlertTriangle size={18} aria-hidden="true" />
            )}
            <div>
              <strong>
                {stats.anomalyBlocks === 0
                  ? 'Every eligible canonical block is fully verified'
                  : `${stats.anomalyBlocks} canonical block${stats.anomalyBlocks === 1 ? '' : 's'} need attention`}
              </strong>
              <p>
                {stats.scopeNotice}{' '}
                {stats.lastVerifiedAt
                  ? `Last verification ${relativeTime(stats.lastVerifiedAt)}.`
                  : 'No verification time is available.'}
              </p>
            </div>
          </div>
          {stats.anomalyBlocks > 0 ? (
            <div className="anomaly-breakdown">
              <Metric
                label="Parent disagreement"
                value={stats.disagreementParentBlocks.toLocaleString()}
              />
              <Metric
                label="Parent not found"
                value={stats.notFoundParentBlocks.toLocaleString()}
              />
              <Metric
                label="Parent unavailable"
                value={stats.unavailableParentBlocks.toLocaleString()}
              />
              <Metric
                label="Orphaned parent"
                value={stats.orphanedParentBlocks.toLocaleString()}
              />
            </div>
          ) : null}
        </div>
      ) : null}
    </InlineResource>
  );
}

export function NetworkAnalytics() {
  const [history, setHistory] =
    useState<Resource<NetworkHistory>>(emptyResource);
  const [pools, setPools] = useState<Resource<ValuePoolHistory>>(emptyResource);

  useEffect(() => {
    const controller = new AbortController();
    void loadNetworkHistory(controller.signal).then(
      (envelope) => setHistory({ envelope, error: '', loading: false }),
      () =>
        setHistory({
          envelope: null,
          error: 'Canonical block history is unavailable.',
          loading: false,
        }),
    );
    void loadValuePoolHistory(controller.signal).then(
      (envelope) => setPools({ envelope, error: '', loading: false }),
      () =>
        setPools({
          envelope: null,
          error: 'Value-pool history is unavailable.',
          loading: false,
        }),
    );
    return () => controller.abort();
  }, []);

  const network = history.envelope?.data;
  const poolHistory = pools.envelope?.data;
  const spacingPoints: ChartPoint[] =
    network?.points.map((point) => ({
      x: point.height,
      label: `Block #${point.height.toLocaleString()}`,
      values: {
        spacing:
          point.spacingSeconds === null
            ? null
            : {
                value: point.spacingSeconds,
                label: `${point.spacingSeconds.toLocaleString()}s`,
              },
      },
    })) ?? [];
  const difficultyPoints: ChartPoint[] =
    network?.points.map((point) => ({
      x: point.height,
      label: `Block #${point.height.toLocaleString()}`,
      values: {
        difficulty: numericValue(point.difficulty, point.difficulty),
      },
    })) ?? [];
  const poolPoints: ChartPoint[] =
    poolHistory?.points.map((point) => ({
      x: point.height,
      label: `Block #${point.height.toLocaleString()}`,
      values: {
        transparent:
          point.transparent.monitored === true
            ? amountValue(point.transparent.chainValue)
            : null,
        ironwood:
          point.ironwood.monitored === true
            ? amountValue(point.ironwood.chainValue)
            : null,
      },
    })) ?? [];
  const latestPools = poolHistory?.points.at(-1);

  return (
    <div className="space-y-6">
      <InlineResource resource={history}>
        {network ? (
          <div className="chart-grid">
            <ExplorerLineChart
              title="Block interval"
              description={`Canonical time between blocks through height ${network.asOfHeight ?? '—'}. Testnet bursts and pauses are shown as recorded.`}
              points={spacingPoints}
              series={[
                {
                  key: 'spacing',
                  label: 'Observed interval',
                  color: 'var(--brand)',
                },
              ]}
              reference={{
                value: network.targetSpacingSeconds,
                label: 'Target',
                valueLabel: `${network.targetSpacingSeconds}s`,
              }}
            />
            <ExplorerLineChart
              title="Mining difficulty"
              description={`Canonical compact-target difficulty through height ${network.asOfHeight ?? '—'}. This is not a hash-rate estimate.`}
              points={difficultyPoints}
              series={[
                {
                  key: 'difficulty',
                  label: 'Difficulty',
                  color: 'var(--info)',
                },
              ]}
            />
          </div>
        ) : null}
      </InlineResource>

      <InlineResource resource={pools}>
        {poolHistory ? (
          <div className="space-y-4">
            <div className="analytics-summary-grid analytics-summary-grid-three">
              <Metric
                label="Transparent pool"
                value={monitoredAmount(
                  latestPools?.transparent.chainValue ?? null,
                  latestPools?.transparent.monitored ?? null,
                )}
              />
              <Metric
                label="Ironwood pool"
                value={monitoredAmount(
                  latestPools?.ironwood.chainValue ?? null,
                  latestPools?.ironwood.monitored ?? null,
                )}
              />
              <Metric
                label="Unexpected pool samples"
                value={poolHistory.unexpectedPoolSamples.toLocaleString()}
              />
            </div>
            {poolHistory.unexpectedPoolSamples > 0 ? (
              <WarningNotice>
                Legacy or unknown value-pool telemetry is non-zero or marked as
                monitored. Inspect the indexed blocks before relying on these
                totals.
              </WarningNotice>
            ) : null}
            <ExplorerLineChart
              title="Public value-pool totals"
              description={`${poolHistory.scopeNotice} Values are shown only when the node reports that pool as monitored.`}
              points={poolPoints}
              series={[
                {
                  key: 'transparent',
                  label: 'Transparent',
                  color: 'var(--brand)',
                },
                { key: 'ironwood', label: 'Ironwood', color: 'var(--info)' },
              ]}
            />
          </div>
        ) : null}
      </InlineResource>
    </div>
  );
}

export function AddressesPage() {
  const [stats, setStats] =
    useState<Resource<TransparentAddressStats>>(emptyResource);
  const [ranking, setRanking] =
    useState<Resource<TransparentRichList>>(emptyResource);

  useEffect(() => {
    const controller = new AbortController();
    void loadTransparentAddressStats(controller.signal).then(
      (envelope) => setStats({ envelope, error: '', loading: false }),
      () =>
        setStats({
          envelope: null,
          error: 'Transparent-address statistics are unavailable.',
          loading: false,
        }),
    );
    void loadTransparentRichList(controller.signal).then(
      (envelope) => setRanking({ envelope, error: '', loading: false }),
      () =>
        setRanking({
          envelope: null,
          error: 'Transparent-balance ranking is unavailable.',
          loading: false,
        }),
    );
    return () => controller.abort();
  }, []);

  const addressStats = stats.envelope?.data;
  const richList = ranking.envelope?.data;
  const addressPoints: ChartPoint[] =
    addressStats?.points.map((point) => ({
      x: point.height,
      label: `Block #${point.height.toLocaleString()}`,
      values: {
        total: {
          value: point.totalSeenAddresses,
          label: point.totalSeenAddresses.toLocaleString(),
        },
        active: {
          value: point.activeAddresses,
          label: point.activeAddresses.toLocaleString(),
        },
        new: {
          value: point.newAddresses,
          label: point.newAddresses.toLocaleString(),
        },
      },
    })) ?? [];

  return (
    <div className="space-y-6">
      <header className="page-intro">
        <div>
          <div className="eyebrow">Public chain data</div>
          <h1 className="page-title">Transparent balances</h1>
          <p className="page-description">
            Decoded transparent addresses on the canonical Wcash chain. Shielded
            identities and balances are not observable.
          </p>
        </div>
      </header>
      <SearchBox />
      <PrivacyBoundaryNotice />
      <InlineResource resource={stats}>
        {addressStats ? (
          <div className="space-y-4">
            <div className="analytics-summary-grid">
              <Metric
                label="Funded addresses"
                value={addressStats.fundedAddressCount.toLocaleString()}
              />
              <Metric
                label="Seen addresses"
                value={addressStats.seenAddressCount.toLocaleString()}
              />
              <Metric
                label="Addressed balance"
                value={formatAmount(addressStats.addressedBalance)}
              />
              <Metric
                label="Undecoded transparent"
                value={monitoredAmount(
                  addressStats.addresslessOrUndecodedBalance,
                  addressStats.transparentPoolMonitored,
                )}
              />
            </div>
            <ExplorerLineChart
              title="Transparent address activity"
              description={`${addressStats.scopeNotice} Counts are grouped by canonical block.`}
              points={addressPoints}
              series={[
                { key: 'total', label: 'Total seen', color: 'var(--brand)' },
                { key: 'active', label: 'Active', color: 'var(--info)' },
                { key: 'new', label: 'New', color: 'var(--warning)' },
              ]}
            />
          </div>
        ) : null}
      </InlineResource>
      <section>
        <div className="section-heading">
          <h2>Transparent rich list</h2>
          {richList?.asOfHeight === null ||
          richList?.asOfHeight === undefined ? null : (
            <span className="eyebrow">
              Through block #{richList.asOfHeight.toLocaleString()}
            </span>
          )}
        </div>
        <InlineResource resource={ranking}>
          {richList ? <TransparentBalanceTable ranking={richList} /> : null}
        </InlineResource>
      </section>
    </div>
  );
}

export function AddressBalanceChart({ address }: { address: AddressDetail }) {
  const points: ChartPoint[] = [...address.activity]
    .sort(
      (left, right) =>
        left.blockHeight - right.blockHeight || left.position - right.position,
    )
    .map((activity) => ({
      x: activity.blockHeight,
      label: `Block #${activity.blockHeight.toLocaleString()}`,
      values: { balance: amountValue(activity.balanceAfter) },
    }));

  return (
    <ExplorerLineChart
      title="Transparent balance history"
      description="Running balance after each canonical transparent transaction involving this address. Shielded activity is outside this view."
      points={points}
      series={[{ key: 'balance', label: 'Balance', color: 'var(--brand)' }]}
    />
  );
}

export function WcashValuePools({ pools }: { pools: ValuePool[] }) {
  const expected = ['transparent', 'ironwood'].map(
    (id) => pools.find((pool) => pool.id === id) ?? null,
  );
  const unexpected = pools.filter(
    (pool) =>
      !['transparent', 'ironwood'].includes(pool.id) &&
      (pool.monitored === true ||
        amountIsNonZero(pool.chainValue) ||
        amountIsNonZero(pool.valueDelta)),
  );

  return (
    <div className="space-y-3">
      {unexpected.length ? (
        <WarningNotice>
          Unexpected value-pool telemetry:{' '}
          {unexpected.map((pool) => pool.id).join(', ')}.
        </WarningNotice>
      ) : null}
      <div className="table-shell">
        <table className="data-table">
          <caption className="sr-only">Wcash public value-pool totals</caption>
          <thead>
            <tr>
              <th>Pool</th>
              <th>Chain value</th>
              <th>Block change</th>
              <th>Node telemetry</th>
            </tr>
          </thead>
          <tbody>
            {expected.map((pool, index) => {
              const id = index === 0 ? 'Transparent' : 'Ironwood';
              const monitored = pool?.monitored === true;
              return (
                <tr key={id}>
                  <td data-label="Pool">{id}</td>
                  <td data-label="Chain value" className="mono">
                    {monitored
                      ? formatNullableAmount(pool?.chainValue ?? null)
                      : 'Not monitored by node'}
                  </td>
                  <td data-label="Block change" className="mono">
                    {monitored
                      ? formatNullableAmount(pool?.valueDelta ?? null)
                      : 'Not monitored by node'}
                  </td>
                  <td data-label="Node telemetry">
                    <span
                      className={`status-label ${
                        monitored ? 'status-label-success' : 'status-label-info'
                      }`}
                    >
                      <span className="status-dot" aria-hidden="true" />
                      {monitored ? 'Monitored' : 'Unavailable'}
                    </span>
                  </td>
                </tr>
              );
            })}
          </tbody>
        </table>
      </div>
    </div>
  );
}

function TransparentBalanceTable({
  ranking,
}: {
  ranking: TransparentRichList;
}) {
  if (!ranking.addresses.length) {
    return (
      <div className="panel p-5 text-sm text-[var(--muted)]">
        No funded transparent addresses.
      </div>
    );
  }
  return (
    <div className="table-shell">
      <table className="data-table">
        <caption className="sr-only">
          Funded transparent Wcash addresses, excluding shielded holdings
        </caption>
        <thead>
          <tr>
            <th>Rank</th>
            <th>Address</th>
            <th>Transparent balance</th>
            <th>Share of transparent pool</th>
            <th>UTXOs</th>
            <th>Transactions</th>
            <th>Last activity</th>
          </tr>
        </thead>
        <tbody>
          {ranking.addresses.map((entry) => (
            <tr key={entry.address}>
              <td data-label="Rank" className="mono">
                #{entry.rank}
              </td>
              <td data-label="Address" data-wide="true">
                <Link href={`/address/${entry.address}`} className="hash block">
                  {middleEllipsis(entry.address, 30)}
                </Link>
              </td>
              <td
                data-label="Transparent balance"
                className="mono whitespace-nowrap"
              >
                {formatAmount(entry.balance)}
              </td>
              <td data-label="Share of transparent pool" className="mono">
                {ranking.transparentPoolMonitored === true &&
                entry.transparentPoolSharePercent !== null
                  ? `${trimPercent(entry.transparentPoolSharePercent)}%`
                  : 'Not monitored'}
              </td>
              <td data-label="UTXOs" className="mono">
                {entry.utxoCount.toLocaleString()}
              </td>
              <td data-label="Transactions" className="mono">
                {entry.transactionCount.toLocaleString()}
              </td>
              <td data-label="Last activity" className="whitespace-nowrap">
                {entry.lastSeenAt
                  ? relativeTime(entry.lastSeenAt)
                  : 'Unavailable'}
              </td>
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}

function PrivacyBoundaryNotice() {
  return (
    <aside className="privacy-boundary">
      <strong>Privacy boundary</strong>
      <p>
        This ranking covers transparent unspent value only. It excludes shielded
        holdings and is not a ranking of all Wcash holders.
      </p>
    </aside>
  );
}

function InlineResource<T>({
  resource,
  children,
}: {
  resource: Resource<T>;
  children: React.ReactNode;
}) {
  if (resource.loading) {
    return (
      <output
        className="skeleton block h-64"
        aria-label="Loading canonical analytics"
      >
        <span className="sr-only">Loading canonical analytics</span>
      </output>
    );
  }
  if (resource.error) {
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
          <p className="mt-1 text-sm leading-6 text-[var(--muted)]">
            {resource.error}
          </p>
        </div>
      </div>
    );
  }
  return <>{children}</>;
}

function Metric({ label, value }: { label: string; value: string }) {
  return (
    <div className="stat-cell">
      <div className="stat-label">{label}</div>
      <div className="stat-value">{value}</div>
    </div>
  );
}

function WarningNotice({ children }: { children: React.ReactNode }) {
  return (
    <div className="panel flex items-start gap-3 border-[var(--warning)] p-4 text-sm">
      <AlertTriangle
        className="mt-0.5 shrink-0 text-[var(--warning)]"
        size={16}
      />
      <p className="leading-6">{children}</p>
    </div>
  );
}

function numericValue(raw: string, label: string) {
  const value = Number(raw);
  return Number.isFinite(value) ? { value, label } : null;
}

function amountValue(amount: Amount | null) {
  return amount ? numericValue(amount.decimal, formatAmount(amount)) : null;
}

function formatAmount(amount: Amount) {
  return `${trimAmount(amount.decimal)} ${amount.symbol}`;
}

function formatNullableAmount(amount: Amount | null) {
  return amount ? formatAmount(amount) : 'Unavailable';
}

function monitoredAmount(amount: Amount | null, monitored: boolean | null) {
  return monitored === true ? formatNullableAmount(amount) : 'Not monitored';
}

function amountIsNonZero(amount: Amount | null) {
  if (!amount) return false;
  const value = Number(amount.decimal);
  return Number.isFinite(value) && value !== 0;
}

function trimPercent(value: string) {
  return value.replace(/\.0+$/, '').replace(/(\.\d*?)0+$/, '$1');
}
