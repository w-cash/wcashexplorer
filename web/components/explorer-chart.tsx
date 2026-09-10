'use client';

import { useId } from 'react';

export type ChartValue = {
  value: number;
  label: string;
};

export type ChartPoint = {
  x: number;
  label: string;
  values: Record<string, ChartValue | null>;
};

export type ChartSeries = {
  key: string;
  label: string;
  color: string;
};

export function ExplorerLineChart({
  title,
  description,
  points,
  series,
  reference,
}: {
  title: string;
  description: string;
  points: ChartPoint[];
  series: ChartSeries[];
  reference?: { value: number; label: string; valueLabel: string };
}) {
  const titleId = useId();
  const descriptionId = useId();
  const width = 960;
  const height = 250;
  const plot = { left: 64, right: 18, top: 20, bottom: 38 };
  const plotWidth = width - plot.left - plot.right;
  const plotHeight = height - plot.top - plot.bottom;
  const visibleValues = points.flatMap((point) =>
    series.flatMap((item) => {
      const value = point.values[item.key];
      return value && Number.isFinite(value.value) ? [value.value] : [];
    }),
  );

  if (!points.length || !visibleValues.length) {
    return (
      <section className="panel chart-panel">
        <ChartHeading title={title} description={description} />
        <div className="chart-empty">No canonical samples available.</div>
      </section>
    );
  }

  const xMin = Math.min(...points.map((point) => point.x));
  const xMax = Math.max(...points.map((point) => point.x));
  const candidateValues = reference
    ? [...visibleValues, reference.value]
    : visibleValues;
  let yMin = Math.min(...candidateValues);
  let yMax = Math.max(...candidateValues);
  const valuesAreNonnegative = yMin >= 0;
  if (yMin === yMax) {
    const padding = Math.max(Math.abs(yMin) * 0.05, 1);
    yMin = valuesAreNonnegative ? 0 : yMin - padding;
    yMax += padding;
  } else {
    const padding = (yMax - yMin) * 0.08;
    yMin = valuesAreNonnegative ? Math.max(0, yMin - padding) : yMin - padding;
    yMax += padding;
  }

  const xPosition = (value: number) =>
    plot.left +
    (xMax === xMin
      ? plotWidth / 2
      : ((value - xMin) / (xMax - xMin)) * plotWidth);
  const yPosition = (value: number) =>
    plot.top + ((yMax - value) / (yMax - yMin)) * plotHeight;
  const gridValues = Array.from(
    { length: 5 },
    (_, index) => yMin + ((yMax - yMin) * index) / 4,
  );
  const paths = series.flatMap((item) =>
    lineSegments(points, item.key).map((segment, index) => ({
      key: `${item.key}-${index}`,
      color: item.color,
      points: segment
        .map((point) => {
          const value = point.values[item.key];
          return value
            ? `${xPosition(point.x).toFixed(2)},${yPosition(value.value).toFixed(2)}`
            : '';
        })
        .filter(Boolean)
        .join(' '),
    })),
  );

  return (
    <section className="panel chart-panel">
      <ChartHeading title={title} description={description} />
      <div className="chart-canvas">
        <svg
          viewBox={`0 0 ${width} ${height}`}
          aria-labelledby={`${titleId} ${descriptionId}`}
          preserveAspectRatio="none"
        >
          <title id={titleId}>{title}</title>
          <desc id={descriptionId}>{description}</desc>
          {gridValues.map((value) => {
            const y = yPosition(value);
            return (
              <g key={value}>
                <line
                  x1={plot.left}
                  x2={width - plot.right}
                  y1={y}
                  y2={y}
                  className="chart-grid-line"
                />
                <text
                  x={plot.left - 9}
                  y={y + 4}
                  textAnchor="end"
                  className="chart-axis-label"
                >
                  {compactNumber(value)}
                </text>
              </g>
            );
          })}
          {reference ? (
            <g>
              <line
                x1={plot.left}
                x2={width - plot.right}
                y1={yPosition(reference.value)}
                y2={yPosition(reference.value)}
                className="chart-reference-line"
              />
              <title>{`${reference.label}: ${reference.valueLabel}`}</title>
            </g>
          ) : null}
          {paths.map((path) => (
            <polyline
              key={path.key}
              points={path.points}
              fill="none"
              stroke={path.color}
              strokeWidth="2.5"
              vectorEffect="non-scaling-stroke"
            />
          ))}
          {points.flatMap((point) =>
            series.flatMap((item) => {
              const value = point.values[item.key];
              if (!value) return [];
              return [
                <circle
                  key={`${point.x}:${item.key}`}
                  cx={xPosition(point.x)}
                  cy={yPosition(value.value)}
                  r="3.25"
                  fill={item.color}
                  className="chart-point"
                >
                  <title>{`${point.label} · ${item.label}: ${value.label}`}</title>
                </circle>,
              ];
            }),
          )}
          <text
            x={plot.left}
            y={height - 10}
            textAnchor="start"
            className="chart-axis-label"
          >
            #{xMin.toLocaleString()}
          </text>
          <text
            x={width - plot.right}
            y={height - 10}
            textAnchor="end"
            className="chart-axis-label"
          >
            #{xMax.toLocaleString()}
          </text>
        </svg>
      </div>
      <div className="chart-legend" aria-label={`${title} legend`}>
        {series.map((item) => {
          const latest = [...points]
            .reverse()
            .map((point) => point.values[item.key])
            .find((value): value is ChartValue => Boolean(value));
          return (
            <div key={item.key} className="chart-legend-item">
              <span
                className="chart-legend-swatch"
                style={{ backgroundColor: item.color }}
                aria-hidden="true"
              />
              <span>{item.label}</span>
              <strong className="mono">
                {latest?.label ?? 'Not monitored'}
              </strong>
            </div>
          );
        })}
        {reference ? (
          <div className="chart-legend-item">
            <span className="chart-legend-rule" aria-hidden="true" />
            <span>{reference.label}</span>
            <strong className="mono">{reference.valueLabel}</strong>
          </div>
        ) : null}
      </div>
    </section>
  );
}

function ChartHeading({
  title,
  description,
}: {
  title: string;
  description: string;
}) {
  return (
    <header className="chart-heading">
      <h2>{title}</h2>
      <p>{description}</p>
    </header>
  );
}

function lineSegments(points: ChartPoint[], key: string) {
  const segments: ChartPoint[][] = [];
  let current: ChartPoint[] = [];
  for (const point of points) {
    if (point.values[key]) {
      current.push(point);
    } else if (current.length) {
      segments.push(current);
      current = [];
    }
  }
  if (current.length) segments.push(current);
  return segments;
}

function compactNumber(value: number) {
  if (!Number.isFinite(value)) return '—';
  return new Intl.NumberFormat('en', {
    notation: Math.abs(value) >= 10_000 ? 'compact' : 'standard',
    maximumFractionDigits: Math.abs(value) < 10 ? 2 : 0,
  }).format(value);
}
