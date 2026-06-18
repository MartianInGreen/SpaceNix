import { clsx, type ClassValue } from "clsx";
import { twMerge } from "tailwind-merge";

export function cn(...inputs: ClassValue[]) {
  return twMerge(clsx(inputs));
}

export function formatBytes(bytes: number | bigint): string {
  const n = typeof bytes === "bigint" ? Number(bytes) : bytes;
  if (!Number.isFinite(n) || n <= 0) return "0 B";
  const units = ["B", "KB", "MB", "GB", "TB"];
  const i = Math.min(Math.floor(Math.log(n) / Math.log(1024)), units.length - 1);
  const value = n / Math.pow(1024, i);
  return `${value.toFixed(value >= 10 || i === 0 ? 0 : 1)} ${units[i]}`;
}

export function formatTimestamp(ts: { microsSinceUnixEpoch?: bigint } | bigint | undefined): string {
  if (ts == null) return "—";
  const micros = typeof ts === "bigint" ? ts : ts.microsSinceUnixEpoch ?? 0n;
  if (!micros) return "—";
  const ms = Number(micros / 1000n);
  return new Date(ms).toLocaleString();
}

export function shortId(id: string | undefined | null, head = 8, tail = 6): string {
  if (!id) return "—";
  if (id.length <= head + tail) return id;
  return `${id.slice(0, head)}…${id.slice(-tail)}`;
}
