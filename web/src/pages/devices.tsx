import * as React from "react";
import { useReducer, useTable } from "spacetimedb/react";
import {
  Laptop,
  Pencil,
  Activity,
  Plus,
  Server,
  ShieldCheck,
  ShieldOff,
  Terminal,
  Cpu,
  MemoryStick,
  HardDrive,
  Network,
  Clock,
} from "lucide-react";

import { reducers, tables } from "@/module_bindings";
import type {
  DeviceMetadata,
  SshEndpointMetadata,
} from "@/module_bindings/types";
import { cn, formatBytes, formatTimestamp } from "@/lib/utils";
import { reportError, reportSuccess } from "@/lib/toast";
import { PageHeader, EmptyState, ConfirmDelete, Spinner, ChipList } from "@/components/common";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Badge } from "@/components/ui/badge";
import { Separator } from "@/components/ui/separator";
import { Switch } from "@/components/ui/switch";
import { Checkbox } from "@/components/ui/checkbox";
import {
  Card,
  CardContent,
  CardHeader,
  CardTitle,
} from "@/components/ui/card";
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";

type MetricRow = {
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
  storageUsedBytes: bigint;
  storageTotalBytes: bigint;
};

function pickLatest(metrics: readonly MetricRow[]): Map<string, MetricRow> {
  const map = new Map<string, MetricRow>();
  for (const m of metrics) {
    const key = String(m.deviceId);
    const current = map.get(key);
    if (!current || m.recordedAt.microsSinceUnixEpoch > current.recordedAt.microsSinceUnixEpoch) {
      map.set(key, m);
    }
  }
  return map;
}

function percent(used: bigint, total: bigint): number {
  if (total === 0n) return 0;
  return Number((used * 10000n) / total) / 100;
}

function fmtPct(n: number): string {
  return `${n.toFixed(1)}%`;
}

function ageSeconds(m: { recordedAt: { microsSinceUnixEpoch: bigint } }): number {
  const now = BigInt(Date.now()) * 1000n;
  const micros = m.recordedAt.microsSinceUnixEpoch;
  if (micros > now) return 0;
  return Number((now - micros) / 1_000_000n);
}

function MetricBar({
  label,
  icon: Icon,
  used,
  total,
  text,
}: {
  label: string;
  icon: React.ComponentType<{ className?: string }>;
  used: bigint;
  total: bigint;
  text?: string;
}) {
  const pct = Math.min(100, Math.max(0, percent(used, total)));
  const danger = pct >= 90;
  return (
    <div className="space-y-1.5">
      <div className="flex items-center justify-between text-xs">
        <span className="flex items-center gap-1.5 text-muted-foreground">
          <Icon className="size-3.5" />
          {label}
        </span>
        <span className="font-mono">
          {text ?? `${formatBytes(used)} / ${formatBytes(total)}`}
        </span>
      </div>
      <div className="h-1.5 w-full overflow-hidden rounded-full bg-muted">
        <div
          className={cn(
            "h-full rounded-full transition-all",
            danger ? "bg-red-500" : pct >= 70 ? "bg-amber-500" : "bg-emerald-500"
          )}
          style={{ width: `${pct}%` }}
        />
      </div>
    </div>
  );
}

function DeviceMetricsCard({ metric }: { metric: MetricRow }) {
  const age = ageSeconds(metric);
  const stale = age > 90;
  return (
    <Card>
      <CardHeader className="flex flex-row items-center justify-between gap-2 space-y-0 pb-3">
        <CardTitle className="flex items-center gap-2 text-base">
          <Activity className="size-4 text-emerald-500" />
          Latest metrics
        </CardTitle>
        <span
          className={cn(
            "text-xs",
            stale ? "text-amber-500" : "text-muted-foreground"
          )}
        >
          {stale ? "stale" : "live"} · {age}s ago
        </span>
      </CardHeader>
      <CardContent className="space-y-3">
        <div className="flex items-baseline gap-2">
          <span className="text-3xl font-semibold tabular-nums">
            {fmtPct(metric.cpuPercent)}
          </span>
          <span className="text-sm text-muted-foreground">CPU</span>
        </div>
        <div className="h-2 w-full overflow-hidden rounded-full bg-muted">
          <div
            className={cn(
              "h-full rounded-full transition-all",
              metric.cpuPercent >= 90
                ? "bg-red-500"
                : metric.cpuPercent >= 70
                  ? "bg-amber-500"
                  : "bg-emerald-500"
            )}
            style={{ width: `${Math.min(100, metric.cpuPercent)}%` }}
          />
        </div>

        <Separator />

        <MetricBar
          label="RAM"
          icon={MemoryStick}
          used={metric.ramUsedBytes}
          total={metric.ramTotalBytes}
        />
        <MetricBar
          label="Swap"
          icon={MemoryStick}
          used={metric.swapUsedBytes}
          total={metric.swapTotalBytes}
        />
        <MetricBar
          label="Storage"
          icon={HardDrive}
          used={metric.storageUsedBytes}
          total={metric.storageTotalBytes}
        />

        <Separator />

        <div className="grid grid-cols-2 gap-3 text-xs">
          <div className="space-y-1">
            <div className="flex items-center gap-1.5 text-muted-foreground">
              <Network className="size-3.5" /> Network RX (cumulative)
            </div>
            <div className="font-mono">{formatBytes(metric.netRxBytes)}</div>
          </div>
          <div className="space-y-1">
            <div className="flex items-center gap-1.5 text-muted-foreground">
              <Network className="size-3.5" /> Network TX (cumulative)
            </div>
            <div className="font-mono">{formatBytes(metric.netTxBytes)}</div>
          </div>
        </div>

        <div className="flex items-center gap-1.5 pt-1 text-[11px] text-muted-foreground">
          <Clock className="size-3" />
          Reported {formatTimestamp(metric.recordedAt)}
        </div>
      </CardContent>
    </Card>
  );
}

function MetricsSummaryCell({ metric }: { metric: MetricRow | undefined }) {
  if (!metric) {
    return <span className="text-xs text-muted-foreground">no reports yet</span>;
  }
  const age = ageSeconds(metric);
  const stale = age > 90;
  const ramPct = percent(metric.ramUsedBytes, metric.ramTotalBytes);
  return (
    <div className="space-y-0.5 text-xs">
      <div className="flex items-center gap-2">
        <Cpu className="size-3 text-muted-foreground" />
        <span className="font-mono">{fmtPct(metric.cpuPercent)}</span>
        <span className="text-muted-foreground">cpu</span>
      </div>
      <div className="flex items-center gap-2">
        <MemoryStick className="size-3 text-muted-foreground" />
        <span className="font-mono">{fmtPct(ramPct)}</span>
        <span className="text-muted-foreground">
          ram · {formatBytes(metric.ramUsedBytes)}
        </span>
      </div>
      <div
        className={cn(
          "text-[10px]",
          stale ? "text-amber-500" : "text-muted-foreground"
        )}
      >
        {stale ? "stale" : "live"} · {age}s ago
      </div>
    </div>
  );
}

export function DevicesPage() {
  const [rows, ready] = useTable(tables.my_devices);
  const [sshEndpointRows] = useTable(tables.my_ssh_endpoints);
  const [metricRows] = useTable(tables.my_device_metrics);
  const registerDevice = useReducer(reducers.registerDevice);
  const renameDevice = useReducer(reducers.renameDevice);
  const setDeviceHostname = useReducer(reducers.setDeviceHostname);
  const touchDevice = useReducer(reducers.touchDevice);
  const deleteDevice = useReducer(reducers.deleteDevice);
  const setSshEndpointDevices = useReducer(reducers.setSshEndpointDevices);
  const setSshEndpointEnabled = useReducer(reducers.setSshEndpointEnabled);

  const [registerOpen, setRegisterOpen] = React.useState(false);
  const [editing, setEditing] = React.useState<DeviceMetadata | null>(null);
  const [sshFor, setSshFor] = React.useState<DeviceMetadata | null>(null);
  const [metricsFor, setMetricsFor] = React.useState<DeviceMetadata | null>(null);

  const devices = React.useMemo(
    () => [...rows].sort((a, b) => Number(b.createdAt.microsSinceUnixEpoch - a.createdAt.microsSinceUnixEpoch)),
    [rows]
  );

  const latestByDevice = React.useMemo(
    () => pickLatest(metricRows as readonly MetricRow[]),
    [metricRows]
  );

  const endpointsForDevice = React.useCallback(
    (deviceId: string) => {
      return [...sshEndpointRows]
        .filter((e) => e.deviceIds.length === 0 || e.deviceIds.includes(deviceId))
        .sort((a, b) => a.name.localeCompare(b.name));
    },
    [sshEndpointRows]
  );

  return (
    <div>
      <PageHeader
        title="Devices"
        description="Devices that receive your files and secrets."
        actions={
          <Button onClick={() => setRegisterOpen(true)} className="gap-2">
            <Plus className="size-4" /> Register device
          </Button>
        }
      />

      <RegisterDeviceDialog
        open={registerOpen}
        onOpenChange={setRegisterOpen}
        onSubmit={async (name, hostname) => {
          await registerDevice({ name, hostname: hostname || undefined });
        }}
      />

      <EditDeviceDialog
        device={editing}
        onOpenChange={(o) => !o && setEditing(null)}
        onRename={async (name) => {
          if (!editing) return;
          await renameDevice({ deviceId: editing.id, name });
        }}
        onSetHostname={async (hostname) => {
          if (!editing) return;
          await setDeviceHostname({ deviceId: editing.id, hostname: hostname || undefined });
        }}
      />

      <DeviceSshDialog
        device={sshFor}
        endpointsForDevice={endpointsForDevice}
        onOpenChange={(o) => !o && setSshFor(null)}
        onSetDevices={async (endpointId, deviceIds) => {
          await setSshEndpointDevices({ id: endpointId, deviceIds });
        }}
        onSetEnabled={async (endpointId, enabled) => {
          await setSshEndpointEnabled({ id: endpointId, enabled });
        }}
      />

      <DeviceMetricsDialog
        device={metricsFor}
        metric={metricsFor ? latestByDevice.get(String(metricsFor.id)) : undefined}
        onOpenChange={(o) => !o && setMetricsFor(null)}
      />

      {!ready ? (
        <div className="flex justify-center p-10 text-muted-foreground">
          <Spinner className="size-5" />
        </div>
      ) : devices.length === 0 ? (
        <EmptyState
          icon={Laptop}
          title="No devices registered"
          description="Register a device to receive files and secrets."
          action={
            <Button onClick={() => setRegisterOpen(true)} className="gap-2">
              <Plus className="size-4" /> Register device
            </Button>
          }
        />
      ) : (
        <div className="rounded-lg border">
          <Table>
            <TableHeader>
              <TableRow>
                <TableHead>Name</TableHead>
                <TableHead>Hostname</TableHead>
                <TableHead>SSH</TableHead>
                <TableHead>Last seen</TableHead>
                <TableHead>Metrics</TableHead>
                <TableHead>Created</TableHead>
                <TableHead className="w-40 text-right">Actions</TableHead>
              </TableRow>
            </TableHeader>
            <TableBody>
              {devices.map((d) => {
                const id = String(d.id);
                const eps = endpointsForDevice(id);
                const enabledCount = eps.filter((e) => e.enabled).length;
                const metric = latestByDevice.get(id);
                return (
                  <TableRow key={id}>
                    <TableCell className="font-medium">{d.name}</TableCell>
                    <TableCell>
                      {d.hostname ? (
                        <span className="font-mono text-xs">{d.hostname}</span>
                      ) : (
                        <span className="text-muted-foreground">—</span>
                      )}
                    </TableCell>
                    <TableCell>
                      <Button
                        variant="ghost"
                        size="sm"
                        className="h-7 gap-1.5"
                        onClick={() => setSshFor(d)}
                        title="Toggle SSH endpoints for this device"
                      >
                        <Server className="size-3.5" />
                        {eps.length === 0 ? (
                          <span className="text-muted-foreground">none</span>
                        ) : (
                          <>
                            <span className="font-mono">{enabledCount}</span>
                            <span className="text-muted-foreground">/ {eps.length}</span>
                          </>
                        )}
                      </Button>
                    </TableCell>
                    <TableCell>
                      {d.lastSeenAt ? (
                        <Badge variant="success" className="gap-1">
                          <Activity className="size-3" />
                          {formatTimestamp(d.lastSeenAt)}
                        </Badge>
                      ) : (
                        <span className="text-muted-foreground">never</span>
                      )}
                    </TableCell>
                    <TableCell>
                      <Button
                        variant="ghost"
                        size="sm"
                        className="h-auto items-start justify-start px-2 py-1.5 text-left"
                        onClick={() => setMetricsFor(d)}
                        title="View device metrics"
                      >
                        <MetricsSummaryCell metric={metric} />
                      </Button>
                    </TableCell>
                    <TableCell className="text-muted-foreground">{formatTimestamp(d.createdAt)}</TableCell>
                    <TableCell className="text-right">
                      <div className="flex items-center justify-end gap-1">
                        <Button
                          variant="ghost"
                          size="icon"
                          aria-label="Mark seen"
                          onClick={async () => {
                            try {
                              await touchDevice({ deviceId: d.id });
                              reportSuccess(`Marked "${d.name}" as seen.`);
                            } catch (err) {
                              reportError(err);
                            }
                          }}
                        >
                          <Activity className="size-4" />
                        </Button>
                        <Button variant="ghost" size="icon" aria-label="Edit" onClick={() => setEditing(d)}>
                          <Pencil className="size-4" />
                        </Button>
                        <ConfirmDelete
                          title={`Delete device "${d.name}"?`}
                          description="Secrets assigned to this device will keep its id in their device list until you reassign them."
                          onConfirm={async () => {
                            await deleteDevice({ deviceId: d.id });
                            reportSuccess("Device deleted.");
                          }}
                        />
                      </div>
                    </TableCell>
                  </TableRow>
                );
              })}
            </TableBody>
          </Table>
        </div>
      )}

      <div className="mt-4 text-xs text-muted-foreground">
        <ChipList items={[]} empty="Tip: device ids are referenced by secrets when scoping values to specific machines." />
      </div>
    </div>
  );
}

function RegisterDeviceDialog({
  open,
  onOpenChange,
  onSubmit,
}: {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  onSubmit: (name: string, hostname: string) => Promise<void>;
}) {
  const [name, setName] = React.useState("");
  const [hostname, setHostname] = React.useState("");
  const [busy, setBusy] = React.useState(false);

  React.useEffect(() => {
    if (open) {
      setName("");
      setHostname("");
    }
  }, [open]);

  const submit = async () => {
    if (!name.trim()) return;
    setBusy(true);
    try {
      await onSubmit(name.trim(), hostname.trim());
      onOpenChange(false);
      reportSuccess("Device registered.");
    } catch (err) {
      reportError(err);
    } finally {
      setBusy(false);
    }
  };

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>Register a device</DialogTitle>
          <DialogDescription>Give the machine a recognizable name. Hostname is optional.</DialogDescription>
        </DialogHeader>
        <div className="space-y-4">
          <div className="space-y-2">
            <Label htmlFor="dev-name">Name</Label>
            <Input
              id="dev-name"
              placeholder="thinkpad-x1"
              value={name}
              onChange={(e) => setName(e.target.value)}
              maxLength={128}
              autoFocus
            />
          </div>
          <div className="space-y-2">
            <Label htmlFor="dev-host">Hostname (optional)</Label>
            <Input
              id="dev-host"
              placeholder="x1.local"
              value={hostname}
              onChange={(e) => setHostname(e.target.value)}
              maxLength={256}
            />
          </div>
        </div>
        <DialogFooter>
          <Button variant="outline" onClick={() => onOpenChange(false)} disabled={busy}>
            Cancel
          </Button>
          <Button onClick={submit} disabled={busy || !name.trim()}>
            {busy ? <Spinner /> : null}
            Register
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}

function EditDeviceDialog({
  device,
  onOpenChange,
  onRename,
  onSetHostname,
}: {
  device: DeviceMetadata | null;
  onOpenChange: (open: boolean) => void;
  onRename: (name: string) => Promise<void>;
  onSetHostname: (hostname: string) => Promise<void>;
}) {
  const [name, setName] = React.useState("");
  const [hostname, setHostname] = React.useState("");
  const [busy, setBusy] = React.useState(false);

  React.useEffect(() => {
    if (device) {
      setName(device.name);
      setHostname(device.hostname ?? "");
    }
  }, [device]);

  const submit = async () => {
    if (!device) return;
    setBusy(true);
    try {
      if (name.trim() && name.trim() !== device.name) {
        await onRename(name.trim());
      }
      const nextHost = hostname.trim();
      const prevHost = device.hostname ?? "";
      if (nextHost !== prevHost) {
        await onSetHostname(nextHost);
      }
      onOpenChange(false);
      reportSuccess("Device updated.");
    } catch (err) {
      reportError(err);
    } finally {
      setBusy(false);
    }
  };

  return (
    <Dialog open={device !== null} onOpenChange={onOpenChange}>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>Edit device</DialogTitle>
          <DialogDescription>
            {device ? `Device id: ${String(device.id)}` : ""}
          </DialogDescription>
        </DialogHeader>
        <div className="space-y-4">
          <div className="space-y-2">
            <Label htmlFor="edit-dev-name">Name</Label>
            <Input id="edit-dev-name" value={name} onChange={(e) => setName(e.target.value)} maxLength={128} />
          </div>
          <div className="space-y-2">
            <Label htmlFor="edit-dev-host">Hostname</Label>
            <Input id="edit-dev-host" value={hostname} onChange={(e) => setHostname(e.target.value)} maxLength={256} />
          </div>
        </div>
        <Separator />
        <DialogFooter>
          <Button variant="outline" onClick={() => onOpenChange(false)} disabled={busy}>
            Cancel
          </Button>
          <Button onClick={submit} disabled={busy}>
            {busy ? <Spinner /> : null}
            Save
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}

function DeviceSshDialog({
  device,
  endpointsForDevice,
  onOpenChange,
  onSetDevices,
  onSetEnabled,
}: {
  device: DeviceMetadata | null;
  endpointsForDevice: (deviceId: string) => SshEndpointMetadata[];
  onOpenChange: (open: boolean) => void;
  onSetDevices: (endpointId: bigint, deviceIds: string[]) => Promise<void>;
  onSetEnabled: (endpointId: bigint, enabled: boolean) => Promise<void>;
}) {
  const deviceId = device ? String(device.id) : "";
  const applicable = React.useMemo(
    () => (device ? endpointsForDevice(deviceId) : []),
    [device, deviceId, endpointsForDevice]
  );

  const toggleAssignment = async (ep: SshEndpointMetadata) => {
    if (!device) return;
    const isExplicit = ep.deviceIds.includes(deviceId);
    let nextIds: string[];
    if (ep.deviceIds.length === 0) {
      nextIds = [deviceId];
    } else if (isExplicit) {
      nextIds = ep.deviceIds.filter((id) => id !== deviceId);
    } else {
      nextIds = [...ep.deviceIds, deviceId];
    }
    try {
      await onSetDevices(ep.id, nextIds);
    } catch (err) {
      reportError(err);
    }
  };

  const toggleEnabled = async (ep: SshEndpointMetadata, enabled: boolean) => {
    try {
      await onSetEnabled(ep.id, enabled);
    } catch (err) {
      reportError(err);
    }
  };

  return (
    <Dialog open={device !== null} onOpenChange={onOpenChange}>
      <DialogContent className="sm:max-w-xl">
        <DialogHeader>
          <DialogTitle>SSH for {device?.name ?? ""}</DialogTitle>
          <DialogDescription>
            Toggle the SSH endpoints that should sync to this device, and turn each one on or off.
          </DialogDescription>
        </DialogHeader>

        {applicable.length === 0 ? (
          <div className="rounded-md border border-dashed p-4 text-sm text-muted-foreground">
            No SSH endpoints defined. <a className="underline" href="#/ssh">Create one</a> to scope it to this device.
          </div>
        ) : (
          <div className="max-h-80 space-y-1 overflow-y-auto rounded-md border p-2">
            {applicable.map((ep) => {
              const isAllDevices = ep.deviceIds.length === 0;
              const isAssigned = isAllDevices || ep.deviceIds.includes(deviceId);
              return (
                <div
                  key={String(ep.id)}
                  className={cn(
                    "flex items-center gap-2 rounded-md px-2 py-1.5 text-sm",
                    isAssigned ? "bg-accent/40" : "hover:bg-accent/40"
                  )}
                >
                  <Checkbox
                    checked={isAssigned}
                    onCheckedChange={() => toggleAssignment(ep)}
                    aria-label={`Assign ${ep.name} to ${device?.name ?? ""}`}
                  />
                  <div className="flex-1">
                    <div className="flex items-center gap-1.5 font-medium">
                      <Terminal className="size-3.5 text-muted-foreground" />
                      {ep.name}
                      {isAllDevices ? (
                        <span className="text-[10px] text-muted-foreground">(all devices)</span>
                      ) : null}
                    </div>
                    <div className="font-mono text-[11px] text-muted-foreground">
                      {ep.username}@{ep.host}:{ep.port}
                    </div>
                  </div>
                  <Switch
                    checked={ep.enabled}
                    onCheckedChange={(v) => toggleEnabled(ep, Boolean(v))}
                    aria-label={`Toggle ${ep.name}`}
                  />
                  {ep.enabled ? (
                    <ShieldCheck className="size-3.5 text-emerald-500" />
                  ) : (
                    <ShieldOff className="size-3.5 text-muted-foreground" />
                  )}
                </div>
              );
            })}
          </div>
        )}

        <DialogFooter>
          <Button variant="outline" onClick={() => onOpenChange(false)}>
            Close
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}

function DeviceMetricsDialog({
  device,
  metric,
  onOpenChange,
}: {
  device: DeviceMetadata | null;
  metric: MetricRow | undefined;
  onOpenChange: (open: boolean) => void;
}) {
  return (
    <Dialog open={device !== null} onOpenChange={onOpenChange}>
      <DialogContent className="sm:max-w-md">
        <DialogHeader>
          <DialogTitle className="flex items-center gap-2">
            <Laptop className="size-4" />
            {device?.name ?? ""}
          </DialogTitle>
          <DialogDescription>
            {device?.hostname ? (
              <span className="font-mono">{device.hostname}</span>
            ) : (
              "No hostname set"
            )}{" "}
            · id #{device ? String(device.id) : ""}
          </DialogDescription>
        </DialogHeader>
        {metric ? (
          <DeviceMetricsCard metric={metric} />
        ) : (
          <div className="rounded-md border border-dashed p-4 text-sm text-muted-foreground">
            No metrics have been reported for this device yet. The
            <code className="mx-1 rounded bg-muted px-1 py-0.5 font-mono text-xs">spacenix service</code>
            worker must be running on the device to send periodic samples.
          </div>
        )}
        <DialogFooter>
          <Button variant="outline" onClick={() => onOpenChange(false)}>
            Close
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
