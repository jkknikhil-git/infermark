import { useEffect, useMemo, useRef, useState, type KeyboardEvent, type PointerEvent } from "react";
import { formatNumber } from "../summary";

/**
 * One series over time: a 2 px line with a light wash, hairline grid, the latest value in the
 * header and a crosshair tooltip. Charts never share a y-axis: two measures get two charts.
 */
export function LineChart({
  title,
  unit,
  points,
  zero = false,
  minSpan = 0,
  empty = "Waiting for the first reading…",
  height = 112,
  table = false,
  format = formatNumber,
}: {
  title: string;
  unit: string;
  /** [x seconds, y] in time order. */
  points: [number, number][];
  /** Start the y-axis at 0 (speeds), so a small drop doesn't look like a cliff. */
  zero?: boolean;
  /** Smallest y-range drawn, so a 0.3% change in battery charge doesn't look like a cliff. */
  minSpan?: number;
  /** Shown when there are no points yet. */
  empty?: string;
  height?: number;
  /** Offer the values as a table (results pages). */
  table?: boolean;
  /** How the headline, summary and table show a value. */
  format?: (value: number) => string;
}) {
  const box = useRef<HTMLDivElement>(null);
  const width = useWidth(box);
  const [hover, setHover] = useState<number | null>(null);

  const pad = { top: 8, right: 8, bottom: 20, left: 40 };
  const plotW = Math.max(width - pad.left - pad.right, 10);
  const plotH = height - pad.top - pad.bottom;

  const scale = useMemo(() => {
    if (points.length === 0) return null;
    const xs = points.map((p) => p[0]);
    const ys = points.map((p) => p[1]);
    const x0 = xs[0];
    const x1 = Math.max(xs[xs.length - 1], x0 + 1);
    let lo = zero ? Math.min(0, ...ys) : Math.min(...ys);
    let hi = Math.max(...ys);
    if (hi - lo < minSpan) {
      const mid = (hi + lo) / 2;
      lo = zero ? Math.max(Math.min(0, lo), mid - minSpan / 2) : mid - minSpan / 2;
      hi = lo + minSpan;
    }
    const [y0, y1] = niceRange(lo, hi);
    return {
      x0,
      x1,
      y0,
      y1,
      x: (v: number) => pad.left + ((v - x0) / (x1 - x0)) * plotW,
      y: (v: number) => pad.top + plotH - ((v - y0) / (y1 - y0 || 1)) * plotH,
    };
  }, [points, zero, minSpan, plotW, plotH, pad.left, pad.top]);

  const latest = points[points.length - 1];
  const shown = hover !== null ? points[hover] : latest;

  const path = scale ? points.map((p, i) => `${i ? "L" : "M"}${scale.x(p[0]).toFixed(1)},${scale.y(p[1]).toFixed(1)}`).join("") : "";
  const area = scale && points.length > 1 ? `${path}L${scale.x(latest[0]).toFixed(1)},${pad.top + plotH}L${scale.x(points[0][0]).toFixed(1)},${pad.top + plotH}Z` : "";

  const pick = (e: PointerEvent<SVGSVGElement>) => {
    if (!scale) return;
    const r = e.currentTarget.getBoundingClientRect();
    const x = scale.x0 + ((e.clientX - r.left - pad.left) / plotW) * (scale.x1 - scale.x0);
    setHover(nearest(points, x));
  };
  const key = (e: KeyboardEvent<SVGSVGElement>) => {
    if (points.length === 0) return;
    const at = hover ?? points.length - 1;
    if (e.key === "ArrowLeft") setHover(Math.max(at - 1, 0));
    else if (e.key === "ArrowRight") setHover(Math.min(at + 1, points.length - 1));
    else return;
    e.preventDefault();
  };

  const summary = scale
    ? `${title}: latest ${format(latest[1])} ${unit}, range ${format(Math.min(...points.map((p) => p[1])))} to ${format(Math.max(...points.map((p) => p[1])))} ${unit} over ${formatSeconds(scale.x1 - scale.x0)}.`
    : `${title}: ${empty}`;

  return (
    <figure className="chart">
      <figcaption className="row spread">
        <span className="label-large">{title}</span>
        {shown && (
          <span className="chart-value">
            {format(shown[1])}
            {unit && <span className="unit">{unit}</span>}
            {hover !== null && <span className="body-small muted"> at {formatSeconds(shown[0])}</span>}
          </span>
        )}
      </figcaption>
      <div ref={box} className="chart-plot" style={{ height }}>
        {!scale && <span className="body-medium muted chart-empty">{empty}</span>}
        {scale && width > 0 && (
          <svg
            width={width}
            height={height}
            role="img"
            aria-label={summary}
            tabIndex={0}
            onPointerMove={pick}
            onPointerLeave={() => setHover(null)}
            onBlur={() => setHover(null)}
            onKeyDown={key}
          >
            {[scale.y0, scale.y1].map((v) => (
              <g key={v}>
                <line className="chart-grid" x1={pad.left} x2={pad.left + plotW} y1={scale.y(v)} y2={scale.y(v)} />
                <text className="chart-tick" x={pad.left - 6} y={scale.y(v)} textAnchor="end" dominantBaseline="middle">
                  {compact(v)}
                </text>
              </g>
            ))}
            <text className="chart-tick" x={pad.left} y={height - 4}>
              {formatSeconds(scale.x0)}
            </text>
            <text className="chart-tick" x={pad.left + plotW} y={height - 4} textAnchor="end">
              {formatSeconds(scale.x1)}
            </text>
            {area && <path className="chart-area" d={area} />}
            <path className="chart-line" d={path} />
            {hover !== null && (
              <line className="chart-crosshair" x1={scale.x(points[hover][0])} x2={scale.x(points[hover][0])} y1={pad.top} y2={pad.top + plotH} />
            )}
            {shown && <circle className="chart-dot" cx={scale.x(shown[0])} cy={scale.y(shown[1])} r={4} />}
          </svg>
        )}
      </div>
      {table && points.length > 0 && (
        <details className="chart-table">
          <summary className="body-small muted">Show values</summary>
          <table className="body-small">
            <thead>
              <tr>
                <th>Time</th>
                <th>{unit}</th>
              </tr>
            </thead>
            <tbody>
              {thin(points, 60).map(([x, y]) => (
                <tr key={x}>
                  <td>{formatSeconds(x)}</td>
                  <td>{format(y)}</td>
                </tr>
              ))}
            </tbody>
          </table>
        </details>
      )}
    </figure>
  );
}

function useWidth(ref: React.RefObject<HTMLDivElement | null>) {
  const [w, setW] = useState(0);
  useEffect(() => {
    const el = ref.current;
    if (!el) return;
    setW(el.clientWidth);
    const ro = new ResizeObserver(() => setW(el.clientWidth));
    ro.observe(el);
    return () => ro.disconnect();
  }, [ref]);
  return w;
}

function nearest(points: [number, number][], x: number) {
  let best = 0;
  for (let i = 1; i < points.length; i++) {
    if (Math.abs(points[i][0] - x) < Math.abs(points[best][0] - x)) best = i;
  }
  return best;
}

/** Round the y-range outwards to clean numbers; a flat series still gets some height. */
function niceRange(lo: number, hi: number): [number, number] {
  if (hi - lo < 1e-9) {
    const pad = Math.abs(hi) * 0.1 || 1;
    lo -= pad;
    hi += pad;
  }
  const step = 10 ** Math.floor(Math.log10(hi - lo));
  return [Math.floor(lo / step) * step, Math.ceil(hi / step) * step];
}

function compact(v: number) {
  if (Math.abs(v) >= 1000) return `${formatNumber(Math.round(v / 100) / 10)}K`;
  return formatNumber(Math.round(v * 10) / 10);
}

export function formatSeconds(s: number) {
  if (s < 90) return `${Math.round(s)} s`;
  const m = s / 60;
  return m < 90 ? `${formatNumber(Math.round(m * 10) / 10)} min` : `${formatNumber(Math.round(m / 6) / 10)} h`;
}

/** At most `max` evenly spaced points, always keeping the last one. */
function thin(points: [number, number][], max: number) {
  if (points.length <= max) return points;
  const step = points.length / max;
  const out = Array.from({ length: max - 1 }, (_, i) => points[Math.floor(i * step)]);
  return [...out, points[points.length - 1]];
}
