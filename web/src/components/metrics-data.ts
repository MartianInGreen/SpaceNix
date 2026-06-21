export type MetricSample = {
  id: bigint;
  deviceId: bigint;
  recordedAt: { microsSinceUnixEpoch: bigint };
  cpuPercent: number;
  ramUsedBytes: bigint;
  ramTotalBytes: bigint;
  swapUsedBytes: bigint;
  swapTotalBytes: bigint;
  netRxBytes: bigint;
  netTxBytes: bigint;
  storageSyncRootUsedBytes: bigint;
  storageSyncRootTotalBytes: bigint;
  storageSystemUsedBytes: bigint;
  storageSystemTotalBytes: bigint;
  syncRootPath: string;
};

const MAX_HISTORY = 60;

/**
 * Last-`MAX_HISTORY` samples for a single device, in chronological order.
 *
 * SpacetimeDB's `useTable` returns rows in the order the server wrote
 * them (which is the order the metrics reporter produced them), so we
 * just keep the tail of the slice that matches `deviceId`.
 */
export function historyForDevice(
  rows: readonly MetricSample[],
  deviceId: bigint,
): MetricSample[] {
  const out: MetricSample[] = [];
  for (const r of rows) {
    if (r.deviceId === deviceId) {
      out.push(r);
      if (out.length > MAX_HISTORY) out.shift();
    }
  }
  return out;
}
