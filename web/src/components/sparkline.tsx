import * as React from "react";

import { cn } from "@/lib/utils";

export type SparklineProps = {
  values: readonly number[];
  width?: number;
  height?: number;
  /** Upper bound for `values`; values are clipped to [0, max]. */
  max?: number;
  strokeClass?: string;
  fillClass?: string;
  className?: string;
  title?: string;
};

/**
 * Tiny dependency-free sparkline. Renders a single polyline over an
 * optional area fill, with the last point highlighted. Designed to be
 * driven by a `useTable` subscription — the component is pure and just
 * re-renders when its `values` prop changes, which SpacetimeDB does
 * automatically on row insert.
 */
export function Sparkline({
  values,
  width = 120,
  height = 32,
  max = 100,
  strokeClass = "stroke-[var(--chart-1)]",
  fillClass = "fill-[var(--chart-1)]/15",
  className,
  title,
}: SparklineProps) {
  if (values.length === 0) {
    return (
      <svg
        viewBox={`0 0 ${width} ${height}`}
        width={width}
        height={height}
        className={cn("text-muted-foreground/40", className)}
        role="img"
        aria-label={title ?? "no samples"}
      >
        <line
          x1={0}
          y1={height / 2}
          x2={width}
          y2={height / 2}
          stroke="currentColor"
          strokeWidth={1}
          strokeDasharray="2 3"
        />
      </svg>
    );
  }

  // Pad to at least 2 points so the polyline renders something on the
  // very first sample.
  const padded = values.length < 2 ? [values[0]!, values[0]!] : values;
  const clamped = padded.map((v) => Math.max(0, Math.min(max, v)));
  const stepX = padded.length > 1 ? width / (padded.length - 1) : width;
  const points = clamped.map((v, i) => {
    const x = i * stepX;
    const y = height - (v / max) * height;
    return { x, y };
  });

  const linePath = points
    .map((p, i) => (i === 0 ? `M ${p.x} ${p.y}` : `L ${p.x} ${p.y}`))
    .join(" ");
  const areaPath = `${linePath} L ${points[points.length - 1]!.x} ${height} L 0 ${height} Z`;
  const last = points[points.length - 1]!;

  return (
    <svg
      viewBox={`0 0 ${width} ${height}`}
      width={width}
      height={height}
      className={cn("block", className)}
      role="img"
      aria-label={title}
    >
      <path d={areaPath} className={fillClass} />
      <path
        d={linePath}
        fill="none"
        strokeWidth={1.5}
        strokeLinecap="round"
        strokeLinejoin="round"
        className={strokeClass}
      />
      <circle
        cx={last.x}
        cy={last.y}
        r={2}
        className={strokeClass}
        fill="currentColor"
      />
    </svg>
  );
}
