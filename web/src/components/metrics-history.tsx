import * as React from "react";
import { Cpu, MemoryStick, HardDrive, Network } from "lucide-react";

import { Sparkline } from "@/components/sparkline";
import { cn, formatBytes, formatTimestamp } from "@/lib/utils";
import { type MetricSample } from "@/components/metrics-data";

const MAX_HISTORY = 60;

function percent(used: bigint, total: bigint): number {
  if (total === 0n) return 0;
  return Number((used * 10000n) / total) / 100;
}

type Series = {
  label: string;
  icon: React.ComponentType<{ className?: string }>;
  stroke: string;
  values: number[];
  suffix: string;
  currentText: string;
};

function buildSeries(
  samples: readonly MetricSample[],
  latest: MetricSample | undefined,
): Series[] {
  const cpu: number[] = samples.map((s) =>
    Math.max(0, Math.min(100, s.cpuPercent)),
  );
  const ram: number[] = samples.map((s) =>
    Math.max(0, Math.min(100, percent(s.ramUsedBytes, s.ramTotalBytes))),
  );
  const sync: number[] = samples.map((s) =>
    Math.max(
      0,
      Math.min(
        100,
        percent(s.storageSyncRootUsedBytes, s.storageSyncRootTotalBytes),
      ),
    ),
  );
  const sys: number[] = samples.map((s) =>
    Math.max(
      0,
      Math.min(
        100,
        percent(s.storageSystemUsedBytes, s.storageSystemTotalBytes),
      ),
    ),
  );

  return [
    {
      label: "CPU",
      icon: Cpu,
      stroke: "stroke-red-500 dark:stroke-red-400",
      values: cpu,
      suffix: "%",
      currentText: latest ? `${latest.cpuPercent.toFixed(1)}%` : "—",
    },
    {
      label: "RAM",
      icon: MemoryStick,
      stroke: "stroke-emerald-500 dark:stroke-emerald-400",
      values: ram,
      suffix: "%",
      currentText: latest
        ? `${percent(latest.ramUsedBytes, latest.ramTotalBytes).toFixed(1)}% (${formatBytes(latest.ramUsedBytes)} / ${formatBytes(latest.ramTotalBytes)})`
        : "—",
    },
    {
      label: "Storage · sync_root",
      icon: HardDrive,
      stroke: "stroke-cyan-500 dark:stroke-cyan-400",
      values: sync,
      suffix: "%",
      currentText: latest
        ? `${percent(latest.storageSyncRootUsedBytes, latest.storageSyncRootTotalBytes).toFixed(1)}% (${formatBytes(latest.storageSyncRootUsedBytes)} / ${formatBytes(latest.storageSyncRootTotalBytes)})`
        : "—",
    },
    {
      label: "Storage · system",
      icon: HardDrive,
      stroke: "stroke-blue-500 dark:stroke-blue-400",
      values: sys,
      suffix: "%",
      currentText: latest
        ? `${percent(latest.storageSystemUsedBytes, latest.storageSystemTotalBytes).toFixed(1)}% (${formatBytes(latest.storageSystemUsedBytes)} / ${formatBytes(latest.storageSystemTotalBytes)})`
        : "—",
    },
  ];
}

export function DeviceMetricsHistory({
  samples,
  latest,
  className,
  compact = false,
}: {
  samples: readonly MetricSample[];
  latest: MetricSample | undefined;
  className?: string;
  compact?: boolean;
}) {
  const series = React.useMemo(() => buildSeries(samples, latest), [samples, latest]);
  const sparklineWidth = compact ? 96 : 220;
  const sparklineHeight = compact ? 24 : 56;

  if (samples.length === 0) {
    return (
      <div
        className={cn(
          "rounded-md border border-dashed p-4 text-sm text-muted-foreground",
          className,
        )}
      >
        No metrics have been reported for this device yet. The
        <code className="mx-1 rounded bg-muted px-1 py-0.5 font-mono text-xs">
          spacenix service
        </code>
        worker must be running on the device to send periodic samples.
      </div>
    );
  }

  return (
    <div className={cn("flex flex-col gap-2", className)}>
      {series.map((s) => {
        const Icon = s.icon;
        return (
          <div
            key={s.label}
            className="flex items-center gap-3 rounded-md border bg-card px-3 py-2 text-card-foreground"
          >
            <Icon className="size-4 shrink-0 text-muted-foreground" />
            <div className="flex min-w-0 flex-1 flex-col gap-0.5">
              <div className="flex items-center justify-between gap-2 text-xs">
                <span className="font-medium text-muted-foreground">{s.label}</span>
                <span className="truncate font-mono text-[11px] text-foreground">
                  {s.currentText}
                </span>
              </div>
              <Sparkline
                values={s.values}
                width={sparklineWidth}
                height={sparklineHeight}
                strokeClass={s.stroke}
                fillClass={cn(
                  s.stroke
                    .replace("stroke-", "fill-")
                    .replace(/\s.*$/, "")
                    .concat("/15"),
                )}
                title={`${s.label} ${s.suffix}`}
              />
            </div>
          </div>
        );
      })}
      <div className="flex flex-wrap items-center justify-between gap-2 text-[11px] text-muted-foreground">
        <span>
          {samples.length} sample{samples.length === 1 ? "" : "s"} (showing last {Math.min(samples.length, MAX_HISTORY)})
        </span>
        {latest ? <span>last: {formatTimestamp(latest.recordedAt)}</span> : null}
      </div>
    </div>
  );
}

export function NetworkSummary({
  latest,
  speed,
}: {
  latest: MetricSample | undefined;
  speed?: { rxBps: number; txBps: number };
}) {
  if (!latest) return null;
  return (
    <div className="flex flex-wrap items-center gap-3 rounded-md border bg-card px-3 py-2 text-xs text-card-foreground">
      <div className="flex items-center gap-2">
        <Network className="size-4 text-muted-foreground" />
        <span className="font-mono">
          {formatBytes(latest.netRxBytes)}↓ · {formatBytes(latest.netTxBytes)}↑
        </span>
        <span className="text-muted-foreground">cumulative</span>
      </div>
      {speed && (speed.rxBps > 0 || speed.txBps > 0) ? (
        <div className="flex items-center gap-2 border-l pl-3">
          <span className="text-muted-foreground">now</span>
          <span className="font-mono tabular-nums">
            ↓ {formatRate(speed.rxBps)} · ↑ {formatRate(speed.txBps)}
          </span>
        </div>
      ) : null}
    </div>
  );
}

function formatRate(bps: number): string {
  if (!Number.isFinite(bps) || bps <= 0) return "0 B/s";
  const units = ["B/s", "KiB/s", "MiB/s", "GiB/s", "TiB/s"];
  let v = bps;
  let i = 0;
  while (v >= 1024 && i < units.length - 1) {
    v /= 1024;
    i += 1;
  }
  if (i === 0) return `${Math.round(v)} ${units[i]}`;
  return `${v.toFixed(v >= 10 ? 0 : 1)} ${units[i]}`;
}
