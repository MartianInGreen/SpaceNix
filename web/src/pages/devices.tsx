import * as React from "react";
import { useReducer, useTable } from "spacetimedb/react";
import { Laptop, Pencil, Activity, Plus } from "lucide-react";

import { reducers, tables } from "@/module_bindings";
import type { DeviceMetadata } from "@/module_bindings/types";
import { formatTimestamp } from "@/lib/utils";
import { reportError, reportSuccess } from "@/lib/toast";
import { PageHeader, EmptyState, ConfirmDelete, Spinner, ChipList } from "@/components/common";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Badge } from "@/components/ui/badge";
import { Separator } from "@/components/ui/separator";
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
  DialogTrigger,
} from "@/components/ui/dialog";

export function DevicesPage() {
  const [rows, ready] = useTable(tables.my_devices);
  const registerDevice = useReducer(reducers.registerDevice);
  const renameDevice = useReducer(reducers.renameDevice);
  const setDeviceHostname = useReducer(reducers.setDeviceHostname);
  const touchDevice = useReducer(reducers.touchDevice);
  const deleteDevice = useReducer(reducers.deleteDevice);

  const [registerOpen, setRegisterOpen] = React.useState(false);
  const [editing, setEditing] = React.useState<DeviceMetadata | null>(null);

  const devices = React.useMemo(
    () => [...rows].sort((a, b) => Number(b.createdAt.microsSinceUnixEpoch - a.createdAt.microsSinceUnixEpoch)),
    [rows]
  );

  return (
    <div>
      <PageHeader
        title="Devices"
        description="Machines that can receive your synced files, configs, and secrets."
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

      {!ready ? (
        <div className="flex justify-center p-10 text-muted-foreground">
          <Spinner className="size-5" />
        </div>
      ) : devices.length === 0 ? (
        <EmptyState
          icon={Laptop}
          title="No devices registered"
          description="Register a device (e.g. your laptop, a server, or a CI runner) to assign secrets and sync configs to it."
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
                <TableHead>Last seen</TableHead>
                <TableHead>Created</TableHead>
                <TableHead className="w-32 text-right">Actions</TableHead>
              </TableRow>
            </TableHeader>
            <TableBody>
              {devices.map((d) => (
                <TableRow key={String(d.id)}>
                  <TableCell className="font-medium">{d.name}</TableCell>
                  <TableCell>
                    {d.hostname ? (
                      <span className="font-mono text-xs">{d.hostname}</span>
                    ) : (
                      <span className="text-muted-foreground">—</span>
                    )}
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
              ))}
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
