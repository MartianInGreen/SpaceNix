import * as React from "react";
import { useProcedure, useReducer, useTable } from "spacetimedb/react";
import {
  Eye,
  EyeOff,
  KeyRound,
  Plus,
  Copy,
  Check,
  Pencil,
  ShieldCheck,
  MonitorSmartphone,
} from "lucide-react";

import { procedures, reducers, tables } from "@/module_bindings";
import type { DeviceMetadata, SecretMetadata, SecretValue } from "@/module_bindings/types";
import { unwrap } from "@/lib/stdb";
import { cn, formatTimestamp } from "@/lib/utils";
import { reportError, reportSuccess } from "@/lib/toast";
import { PageHeader, EmptyState, ConfirmDelete, Spinner, ChipList } from "@/components/common";
import { TagInput } from "@/components/tag-input";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Textarea } from "@/components/ui/textarea";
import { Badge } from "@/components/ui/badge";
import { Checkbox } from "@/components/ui/checkbox";
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
} from "@/components/ui/dialog";
import {
  Tabs,
  TabsContent,
  TabsList,
  TabsTrigger,
} from "@/components/ui/tabs";

const ENV_PATTERN = /^[A-Za-z0-9_.]+$/;
const PERMISSION_PATTERN = /^[A-Za-z0-9:._\-*]+$/;

function deviceKey(id: bigint | number): string {
  return String(id);
}

export function SecretsPage() {
  const [rows, ready] = useTable(tables.my_secrets);
  const [devices] = useTable(tables.my_devices);

  const setSecret = useReducer(reducers.setSecret);
  const setSecretValue = useReducer(reducers.setSecretValue);
  const setSecretDevices = useReducer(reducers.setSecretDevices);
  const setSecretPermissions = useReducer(reducers.setSecretPermissions);
  const deleteSecret = useReducer(reducers.deleteSecret);
  const revealProc = useProcedure(procedures.revealSecret);

  const [createOpen, setCreateOpen] = React.useState(false);
  const [editing, setEditing] = React.useState<SecretMetadata | null>(null);

  const deviceById = React.useMemo(() => {
    const m = new Map<string, DeviceMetadata>();
    for (const d of devices) m.set(deviceKey(d.id), d);
    return m;
  }, [devices]);

  const secrets = React.useMemo(
    () => [...rows].sort((a, b) => a.env.localeCompare(b.env)),
    [rows]
  );

  const reveal = React.useCallback(
    async (id: bigint): Promise<SecretValue | undefined> => {
      const res = await revealProc({ id });
      return unwrap<SecretValue | undefined>(res);
    },
    [revealProc]
  );

  return (
    <div>
      <PageHeader
        title="Secrets"
        description="Environment secrets scoped to devices and permission grants. Values are revealed on demand only."
        actions={
          <Button onClick={() => setCreateOpen(true)} className="gap-2">
            <Plus className="size-4" /> New secret
          </Button>
        }
      />

      <SecretDialog
        mode="create"
        open={createOpen}
        onOpenChange={setCreateOpen}
        devices={devices as readonly DeviceMetadata[]}
        deviceById={deviceById}
        onSubmit={async (env, value, deviceIds, permissions) => {
          await setSecret({ env, value, deviceIds, permissions });
        }}
      />

      <SecretDialog
        mode="edit"
        open={editing !== null}
        secret={editing}
        devices={devices as readonly DeviceMetadata[]}
        deviceById={deviceById}
        onOpenChange={(o) => !o && setEditing(null)}
        reveal={reveal}
        onSetValue={async (value) => {
          if (!editing) return;
          await setSecretValue({ id: editing.id, value });
        }}
        onSetDevices={async (deviceIds) => {
          if (!editing) return;
          await setSecretDevices({ id: editing.id, deviceIds });
        }}
        onSetPermissions={async (permissions) => {
          if (!editing) return;
          await setSecretPermissions({ id: editing.id, permissions });
        }}
      />

      {!ready ? (
        <div className="flex justify-center p-10 text-muted-foreground">
          <Spinner className="size-5" />
        </div>
      ) : secrets.length === 0 ? (
        <EmptyState
          icon={KeyRound}
          title="No secrets yet"
          description="Store API tokens, passwords, or any environment value and scope it to specific devices and permission grants."
          action={
            <Button onClick={() => setCreateOpen(true)} className="gap-2">
              <Plus className="size-4" /> New secret
            </Button>
          }
        />
      ) : (
        <div className="rounded-lg border">
          <Table>
            <TableHeader>
              <TableRow>
                <TableHead>Env</TableHead>
                <TableHead>Devices</TableHead>
                <TableHead>Permissions</TableHead>
                <TableHead>Updated</TableHead>
                <TableHead className="w-40 text-right">Actions</TableHead>
              </TableRow>
            </TableHeader>
            <TableBody>
              {secrets.map((s) => (
                <TableRow key={String(s.id)}>
                  <TableCell className="font-mono font-medium">{s.env}</TableCell>
                  <TableCell>
                    {s.deviceIds.length === 0 ? (
                      <span className="text-muted-foreground">all</span>
                    ) : (
                      <DeviceChips ids={s.deviceIds} deviceById={deviceById} />
                    )}
                  </TableCell>
                  <TableCell>
                    {s.permissions.length === 0 ? (
                      <span className="text-muted-foreground">—</span>
                    ) : (
                      <ChipList items={s.permissions} />
                    )}
                  </TableCell>
                  <TableCell className="text-muted-foreground">{formatTimestamp(s.updatedAt)}</TableCell>
                  <TableCell className="text-right">
                    <div className="flex items-center justify-end gap-1">
                      <RevealButton id={s.id} reveal={reveal} />
                      <Button variant="ghost" size="icon" aria-label="Edit" onClick={() => setEditing(s)}>
                        <Pencil className="size-4" />
                      </Button>
                      <ConfirmDelete
                        title={`Delete secret "${s.env}"?`}
                        description="This permanently removes the secret value. Devices referencing it will no longer receive it."
                        onConfirm={async () => {
                          await deleteSecret({ id: s.id });
                          reportSuccess("Secret deleted.");
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
    </div>
  );
}

function DeviceChips({
  ids,
  deviceById,
}: {
  ids: string[];
  deviceById: Map<string, DeviceMetadata>;
}) {
  return (
    <div className="flex flex-wrap gap-1">
      {ids.map((id) => {
        const d = deviceById.get(id);
        return (
          <span
            key={id}
            className="inline-flex items-center gap-1 rounded-md border bg-muted/40 px-1.5 py-0.5 text-[11px]"
            title={d ? `device id ${id}` : `device id ${id} (unregistered)`}
          >
            <MonitorSmartphone className="size-3 text-muted-foreground" />
            {d ? d.name : <span className="font-mono">{id}</span>}
          </span>
        );
      })}
    </div>
  );
}

function RevealButton({
  id,
  reveal,
}: {
  id: bigint;
  reveal: (id: bigint) => Promise<SecretValue | undefined>;
}) {
  const [open, setOpen] = React.useState(false);
  const [value, setValue] = React.useState<string | null>(null);
  const [loading, setLoading] = React.useState(false);
  const [copied, setCopied] = React.useState(false);

  const load = async () => {
    setLoading(true);
    try {
      const v = await reveal(id);
      setValue(v?.value ?? null);
    } catch (err) {
      reportError(err);
    } finally {
      setLoading(false);
    }
  };

  const copy = async () => {
    if (value == null) return;
    try {
      await navigator.clipboard.writeText(value);
      setCopied(true);
      setTimeout(() => setCopied(false), 1500);
    } catch {
      reportError(new Error("Clipboard unavailable"));
    }
  };

  return (
    <Dialog
      open={open}
      onOpenChange={(o) => {
        setOpen(o);
        if (!o) {
          setValue(null);
          setCopied(false);
        }
      }}
    >
      <Button
        variant="ghost"
        size="icon"
        aria-label="Reveal value"
        onClick={async () => {
          setOpen(true);
          await load();
        }}
      >
        <Eye className="size-4" />
      </Button>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>Reveal secret value</DialogTitle>
          <DialogDescription>
            The value is fetched on demand and shown here only. Copy it somewhere safe.
          </DialogDescription>
        </DialogHeader>
        <div className="space-y-2">
          {loading ? (
            <div className="flex items-center gap-2 text-sm text-muted-foreground">
              <Spinner /> Loading…
            </div>
          ) : value === null ? (
            <p className="text-sm text-muted-foreground">No value stored.</p>
          ) : (
            <div className="flex items-start gap-2">
              <code className="block flex-1 break-all rounded-md border bg-muted/40 p-3 font-mono text-sm">
                {value}
              </code>
              <Button variant="outline" size="icon" onClick={copy} aria-label="Copy">
                {copied ? <Check className="size-4 text-emerald-500" /> : <Copy className="size-4" />}
              </Button>
            </div>
          )}
        </div>
        <DialogFooter>
          <Button variant="outline" onClick={() => setOpen(false)}>
            Close
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}

function SecretDialog({
  mode,
  open,
  onOpenChange,
  secret,
  devices,
  deviceById,
  reveal,
  onSubmit,
  onSetValue,
  onSetDevices,
  onSetPermissions,
}: {
  mode: "create" | "edit";
  open: boolean;
  onOpenChange: (open: boolean) => void;
  secret?: SecretMetadata | null;
  devices: readonly DeviceMetadata[];
  deviceById: Map<string, DeviceMetadata>;
  reveal?: (id: bigint) => Promise<SecretValue | undefined>;
  onSubmit?: (env: string, value: string, deviceIds: string[], permissions: string[]) => Promise<void>;
  onSetValue?: (value: string) => Promise<void>;
  onSetDevices?: (deviceIds: string[]) => Promise<void>;
  onSetPermissions?: (permissions: string[]) => Promise<void>;
}) {
  const isEdit = mode === "edit";
  const [env, setEnv] = React.useState("");
  const [value, setValue] = React.useState("");
  const [deviceIds, setDeviceIds] = React.useState<string[]>([]);
  const [permissions, setPermissions] = React.useState<string[]>([]);
  const [busy, setBusy] = React.useState(false);
  const [tab, setTab] = React.useState("value");
  const [revealed, setRevealed] = React.useState<string | null>(null);

  React.useEffect(() => {
    if (open) {
      if (isEdit && secret) {
        setEnv(secret.env);
        setValue("");
        setDeviceIds(secret.deviceIds);
        setPermissions(secret.permissions);
        setTab("value");
        setRevealed(null);
      } else {
        setEnv("");
        setValue("");
        setDeviceIds([]);
        setPermissions([]);
        setTab("value");
        setRevealed(null);
      }
    }
  }, [open, isEdit, secret]);

  const toggleDevice = (id: string) => {
    setDeviceIds((prev) => (prev.includes(id) ? prev.filter((x) => x !== id) : [...prev, id]));
  };

  const save = async () => {
    if (!ENV_PATTERN.test(env)) {
      reportError(new Error("Env name must match [A-Za-z0-9_.]"));
      return;
    }
    if (isEdit) {
      setBusy(true);
      try {
        if (value) await onSetValue?.(value);
        await onSetDevices?.(deviceIds);
        await onSetPermissions?.(permissions);
        onOpenChange(false);
        reportSuccess("Secret updated.");
      } catch (err) {
        reportError(err);
      } finally {
        setBusy(false);
      }
    } else {
      if (!value) {
        reportError(new Error("Value cannot be empty"));
        return;
      }
      setBusy(true);
      try {
        await onSubmit?.(env, value, deviceIds, permissions);
        onOpenChange(false);
        reportSuccess("Secret created.");
      } catch (err) {
        reportError(err);
      } finally {
        setBusy(false);
      }
    }
  };

  const loadRevealed = async () => {
    if (!reveal || !secret) return;
    try {
      const v = await reveal(secret.id);
      setRevealed(v?.value ?? null);
    } catch (err) {
      reportError(err);
    }
  };

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="sm:max-w-lg">
        <DialogHeader>
          <DialogTitle>{isEdit ? `Edit secret · ${secret?.env ?? ""}` : "New secret"}</DialogTitle>
          <DialogDescription>
            {isEdit
              ? "Update the value, scope it to devices, and manage permission grants."
              : "Store a secret value and scope it to specific devices and permissions."}
          </DialogDescription>
        </DialogHeader>

        <div className="space-y-4">
          <div className="space-y-2">
            <Label htmlFor="secret-env">Env name</Label>
            <Input
              id="secret-env"
              value={env}
              onChange={(e) => setEnv(e.target.value)}
              disabled={isEdit}
              placeholder="DATABASE_URL"
              className="font-mono"
            />
            {isEdit ? (
              <p className="text-[11px] text-muted-foreground">Env name is fixed after creation.</p>
            ) : null}
          </div>

          {isEdit ? (
            <Tabs value={tab} onValueChange={setTab}>
              <TabsList className="grid w-full grid-cols-3">
                <TabsTrigger value="value">Value</TabsTrigger>
                <TabsTrigger value="devices">Devices</TabsTrigger>
                <TabsTrigger value="perms">Permissions</TabsTrigger>
              </TabsList>
              <TabsContent value="value" className="mt-3 space-y-2">
                <div className="flex items-center justify-between">
                  <Label htmlFor="secret-value">New value</Label>
                  <Button variant="ghost" size="sm" className="h-7 gap-1" onClick={loadRevealed}>
                    <Eye className="size-3.5" /> Reveal current
                  </Button>
                </div>
                <Textarea
                  id="secret-value"
                  value={value}
                  onChange={(e) => setValue(e.target.value)}
                  placeholder="Leave blank to keep the current value unchanged."
                  className="font-mono text-sm"
                />
                {revealed !== null ? (
                  <div className="flex items-start gap-2 rounded-md border bg-muted/30 p-2">
                    <code className="block flex-1 break-all font-mono text-xs">{revealed}</code>
                    <EyeOff className="size-3.5 mt-0.5 text-muted-foreground" />
                  </div>
                ) : null}
              </TabsContent>
              <TabsContent value="devices" className="mt-3">
                <DevicePicker devices={devices} selected={deviceIds} onToggle={toggleDevice} deviceById={deviceById} />
              </TabsContent>
              <TabsContent value="perms" className="mt-3 space-y-2">
                <Label>Permissions</Label>
                <TagInput values={permissions} onChange={setPermissions} pattern={PERMISSION_PATTERN} />
              </TabsContent>
            </Tabs>
          ) : (
            <div className="space-y-4">
              <div className="space-y-2">
                <Label htmlFor="secret-value">Value</Label>
                <Textarea
                  id="secret-value"
                  value={value}
                  onChange={(e) => setValue(e.target.value)}
                  placeholder="super-secret-value"
                  className="font-mono text-sm"
                />
              </div>
              <Separator />
              <div className="space-y-2">
                <Label>Devices (optional)</Label>
                <DevicePicker devices={devices} selected={deviceIds} onToggle={toggleDevice} deviceById={deviceById} />
              </div>
              <div className="space-y-2">
                <Label>Permissions (optional)</Label>
                <TagInput values={permissions} onChange={setPermissions} pattern={PERMISSION_PATTERN} />
              </div>
            </div>
          )}
        </div>

        <DialogFooter>
          <Button variant="outline" onClick={() => onOpenChange(false)} disabled={busy}>
            Cancel
          </Button>
          <Button onClick={save} disabled={busy || !env}>
            {busy ? <Spinner /> : null}
            {isEdit ? "Save" : "Create secret"}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}

function DevicePicker({
  devices,
  selected,
  onToggle,
  deviceById,
}: {
  devices: readonly DeviceMetadata[];
  selected: string[];
  onToggle: (id: string) => void;
  deviceById: Map<string, DeviceMetadata>;
}) {
  if (devices.length === 0) {
    return (
      <div className="rounded-md border border-dashed p-4 text-sm text-muted-foreground">
        No devices registered. The secret will apply to all devices until you{" "}
        <a className="underline" href="#/devices">register one</a>.
      </div>
    );
  }
  return (
    <div className="max-h-48 space-y-1 overflow-y-auto rounded-md border p-2">
      {devices.map((d) => {
        const id = deviceKey(d.id);
        const checked = selected.includes(id);
        return (
          <label
            key={id}
            className={cn(
              "flex cursor-pointer items-center gap-2 rounded-md px-2 py-1.5 text-sm hover:bg-accent",
              checked && "bg-accent/60"
            )}
          >
            <Checkbox checked={checked} onCheckedChange={() => onToggle(id)} />
            <MonitorSmartphone className="size-4 text-muted-foreground" />
            <span className="font-medium">{d.name}</span>
            {d.hostname ? <span className="font-mono text-xs text-muted-foreground">{d.hostname}</span> : null}
            <span className="ml-auto font-mono text-[10px] text-muted-foreground">#{id}</span>
          </label>
        );
      })}
      {selected.some((id) => !deviceById.has(id)) ? (
        <div className="border-t pt-2 text-[11px] text-muted-foreground">
          Unregistered device ids: {selected.filter((id) => !deviceById.has(id)).join(", ")}
        </div>
      ) : null}
    </div>
  );
}
