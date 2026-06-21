import * as React from "react";
import { useProcedure, useReducer, useTable } from "spacetimedb/react";
import { useNavigate } from "react-router-dom";
import {
  Eye,
  EyeOff,
  Terminal,
  Plus,
  Copy,
  Check,
  Pencil,
  KeyRound,
  Server,
  ShieldOff,
  ShieldCheck,
  MonitorSmartphone,
  Network,
  Play,
} from "lucide-react";

import { procedures, reducers, tables } from "@/module_bindings";
import type {
  DeviceMetadata,
  SshEndpointMetadata,
  SshKeyMetadata,
  SshKeyValue,
} from "@/module_bindings/types";
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
import { Switch } from "@/components/ui/switch";
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
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";

const TAG_PATTERN = /^[A-Za-z0-9:._\-*]+$/;

function deviceKey(id: bigint | number): string {
  return String(id);
}

export function SshPage() {
  const navigate = useNavigate();
  const [keyRows, keysReady] = useTable(tables.my_ssh_keys);
  const [endpointRows, endpointsReady] = useTable(tables.my_ssh_endpoints);
  const [devices] = useTable(tables.my_devices);
  const [relayDeviceRows] = useTable(tables.my_ssh_relay_device);
  const relayDevice = relayDeviceRows[0];

  const setSshKey = useReducer(reducers.setSshKey);
  const setSshKeyValue = useReducer(reducers.setSshKeyValue);
  const setSshKeyDevices = useReducer(reducers.setSshKeyDevices);
  const setSshKeyTags = useReducer(reducers.setSshKeyTags);
  const deleteSshKey = useReducer(reducers.deleteSshKey);

  const setSshEndpoint = useReducer(reducers.setSshEndpoint);
  const updateSshEndpoint = useReducer(reducers.updateSshEndpoint);
  const setSshEndpointDevices = useReducer(reducers.setSshEndpointDevices);
  const setSshEndpointTags = useReducer(reducers.setSshEndpointTags);
  const setSshEndpointEnabled = useReducer(reducers.setSshEndpointEnabled);
  const deleteSshEndpoint = useReducer(reducers.deleteSshEndpoint);
  const openSshRelaySession = useReducer(reducers.openSshRelaySession);

  const revealProc = useProcedure(procedures.revealSshKey);

  const openTerminal = React.useCallback(
    async (ep: SshEndpointMetadata) => {
      if (!relayDevice) {
        reportError(
          new Error(
            "No SSH relay device is set. Pick one on the Devices page first.",
          ),
        );
        return;
      }
      try {
        await openSshRelaySession({
          relayDeviceId: relayDevice.deviceId,
          endpointId: ep.id,
          requesterDeviceId: undefined,
        });
        // The terminal page finds the most recent active session
        // for this endpoint and reads the auth token from the
        // same row. The token never leaves the browser's STDB
        // client — it never appears in the URL or the UI.
        navigate(
          `/ssh/terminal?endpoint=${encodeURIComponent(String(ep.id))}`,
        );
      } catch (err) {
        reportError(err);
      }
    },
    [relayDevice, openSshRelaySession, navigate],
  );

  const [tab, setTab] = React.useState("keys");
  const [createKeyOpen, setCreateKeyOpen] = React.useState(false);
  const [editingKey, setEditingKey] = React.useState<SshKeyMetadata | null>(null);
  const [createEndpointOpen, setCreateEndpointOpen] = React.useState(false);
  const [editingEndpoint, setEditingEndpoint] = React.useState<SshEndpointMetadata | null>(null);

  const deviceById = React.useMemo(() => {
    const m = new Map<string, DeviceMetadata>();
    for (const d of devices) m.set(deviceKey(d.id), d);
    return m;
  }, [devices]);

  const keys = React.useMemo(
    () => [...keyRows].sort((a, b) => a.name.localeCompare(b.name)),
    [keyRows]
  );
  const endpoints = React.useMemo(
    () => [...endpointRows].sort((a, b) => a.name.localeCompare(b.name)),
    [endpointRows]
  );
  const keysById = React.useMemo(() => {
    const m = new Map<string, SshKeyMetadata>();
    for (const k of keys) m.set(String(k.id), k);
    return m;
  }, [keys]);

  const reveal = React.useCallback(
    async (id: bigint): Promise<SshKeyValue | undefined> => {
      const res = await revealProc({ id });
      return unwrap<SshKeyValue | undefined>(res);
    },
    [revealProc]
  );

  return (
    <div>
      <PageHeader
        title="SSH"
        description="Manage SSH keys and the endpoints that use them. Private keys are only revealed on demand."
        actions={
          tab === "keys" ? (
            <Button onClick={() => setCreateKeyOpen(true)} className="gap-2">
              <Plus className="size-4" /> New key
            </Button>
          ) : (
            <Button
              onClick={() => setCreateEndpointOpen(true)}
              className="gap-2"
              disabled={keys.length === 0}
              title={keys.length === 0 ? "Create an SSH key first" : undefined}
            >
              <Plus className="size-4" /> New endpoint
            </Button>
          )
        }
      />

      <Tabs value={tab} onValueChange={setTab}>
        <TabsList>
          <TabsTrigger value="keys" className="gap-1">
            <KeyRound className="size-3.5" /> Keys
          </TabsTrigger>
          <TabsTrigger value="endpoints" className="gap-1">
            <Server className="size-3.5" /> Endpoints
          </TabsTrigger>
        </TabsList>

        <TabsContent value="keys" className="mt-4">
          <KeysSection
            ready={keysReady}
            keys={keys}
            deviceById={deviceById}
            onReveal={reveal}
            onEdit={setEditingKey}
            onDelete={async (k) => {
              try {
                await deleteSshKey({ id: k.id });
                reportSuccess(`Deleted key "${k.name}".`);
              } catch (err) {
                reportError(err);
              }
            }}
            onCreate={() => setCreateKeyOpen(true)}
          />
        </TabsContent>

        <TabsContent value="endpoints" className="mt-4">
          <EndpointsSection
            ready={endpointsReady}
            endpoints={endpoints}
            keysById={keysById}
            deviceById={deviceById}
            relayDeviceId={relayDevice?.deviceId}
            onCreate={() => setCreateEndpointOpen(true)}
            onEdit={setEditingEndpoint}
            onConnect={openTerminal}
            onToggleEnabled={async (ep, enabled) => {
              try {
                await setSshEndpointEnabled({ id: ep.id, enabled });
              } catch (err) {
                reportError(err);
              }
            }}
            onDelete={async (ep) => {
              try {
                await deleteSshEndpoint({ id: ep.id });
                reportSuccess(`Deleted endpoint "${ep.name}".`);
              } catch (err) {
                reportError(err);
              }
            }}
          />
        </TabsContent>
      </Tabs>

      <KeyDialog
        mode="create"
        open={createKeyOpen}
        onOpenChange={setCreateKeyOpen}
        devices={devices as readonly DeviceMetadata[]}
        deviceById={deviceById}
        onSubmit={async (name, publicKey, privateKey, deviceIds, tags) => {
          await setSshKey({ name, publicKey, privateKey, deviceIds, tags });
        }}
      />

      <KeyDialog
        mode="edit"
        open={editingKey !== null}
        keyRow={editingKey}
        onOpenChange={(o) => !o && setEditingKey(null)}
        devices={devices as readonly DeviceMetadata[]}
        deviceById={deviceById}
        reveal={reveal}
        onSetValue={async (publicKey, privateKey) => {
          if (!editingKey) return;
          await setSshKeyValue({ id: editingKey.id, publicKey, privateKey });
        }}
        onSetDevices={async (deviceIds) => {
          if (!editingKey) return;
          await setSshKeyDevices({ id: editingKey.id, deviceIds });
        }}
        onSetTags={async (tags) => {
          if (!editingKey) return;
          await setSshKeyTags({ id: editingKey.id, tags });
        }}
      />

      <EndpointDialog
        mode="create"
        open={createEndpointOpen}
        onOpenChange={setCreateEndpointOpen}
        keys={keys}
        devices={devices as readonly DeviceMetadata[]}
        deviceById={deviceById}
        onSubmit={async (name, host, port, username, keyId, deviceIds, tags, enabled) => {
          await setSshEndpoint({
            name,
            host,
            port,
            username,
            keyId,
            deviceIds,
            tags,
            enabled,
          });
        }}
      />

      <EndpointDialog
        mode="edit"
        open={editingEndpoint !== null}
        endpoint={editingEndpoint}
        keys={keys}
        devices={devices as readonly DeviceMetadata[]}
        deviceById={deviceById}
        onOpenChange={(o) => !o && setEditingEndpoint(null)}
        onUpdate={async (host, port, username, keyId) => {
          if (!editingEndpoint) return;
          await updateSshEndpoint({ id: editingEndpoint.id, host, port, username, keyId });
        }}
        onSetDevices={async (deviceIds) => {
          if (!editingEndpoint) return;
          await setSshEndpointDevices({ id: editingEndpoint.id, deviceIds });
        }}
        onSetTags={async (tags) => {
          if (!editingEndpoint) return;
          await setSshEndpointTags({ id: editingEndpoint.id, tags });
        }}
      />
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

function DevicePicker({
  devices,
  selected,
  onToggle,
  deviceById,
  emptyHint,
}: {
  devices: readonly DeviceMetadata[];
  selected: string[];
  onToggle: (id: string) => void;
  deviceById: Map<string, DeviceMetadata>;
  emptyHint?: string;
}) {
  if (devices.length === 0) {
    return (
      <div className="rounded-md border border-dashed p-4 text-sm text-muted-foreground">
        {emptyHint ?? (
          <>
            No devices registered. The toggle will apply to all devices until you{" "}
            <a className="underline" href="#/devices">register one</a>.
          </>
        )}
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

function KeysSection({
  ready,
  keys,
  deviceById,
  onReveal,
  onEdit,
  onDelete,
  onCreate,
}: {
  ready: boolean;
  keys: SshKeyMetadata[];
  deviceById: Map<string, DeviceMetadata>;
  onReveal: (id: bigint) => Promise<SshKeyValue | undefined>;
  onEdit: (k: SshKeyMetadata) => void;
  onDelete: (k: SshKeyMetadata) => Promise<void>;
  onCreate: () => void;
}) {
  if (!ready) {
    return (
      <div className="flex justify-center p-10 text-muted-foreground">
        <Spinner className="size-5" />
      </div>
    );
  }
  if (keys.length === 0) {
    return (
      <EmptyState
        icon={KeyRound}
        title="No SSH keys yet"
        description="Store SSH key pairs and scope them to specific devices. Private keys are revealed on demand only."
        action={
          <Button onClick={onCreate} className="gap-2">
            <Plus className="size-4" /> New key
          </Button>
        }
      />
    );
  }
  return (
    <div className="rounded-lg border">
      <Table>
        <TableHeader>
          <TableRow>
            <TableHead>Name</TableHead>
            <TableHead>Fingerprint</TableHead>
            <TableHead>Public key</TableHead>
            <TableHead>Devices</TableHead>
            <TableHead>Tags</TableHead>
            <TableHead>Updated</TableHead>
            <TableHead className="w-40 text-right">Actions</TableHead>
          </TableRow>
        </TableHeader>
        <TableBody>
          {keys.map((k) => (
            <TableRow key={String(k.id)}>
              <TableCell className="font-mono font-medium">{k.name}</TableCell>
              <TableCell>
                <code className="rounded bg-muted/40 px-1.5 py-0.5 font-mono text-[10px]">
                  {k.fingerprint}
                </code>
              </TableCell>
              <TableCell>
                <span className="line-clamp-1 max-w-[260px] font-mono text-xs text-muted-foreground" title={k.publicKey}>
                  {k.publicKey.split(" ")[1] ? `…${k.publicKey.split(" ")[1].slice(-24)}` : k.publicKey}
                </span>
              </TableCell>
              <TableCell>
                {k.deviceIds.length === 0 ? (
                  <span className="text-muted-foreground">all</span>
                ) : (
                  <DeviceChips ids={k.deviceIds} deviceById={deviceById} />
                )}
              </TableCell>
              <TableCell>
                {k.tags.length === 0 ? (
                  <span className="text-muted-foreground">—</span>
                ) : (
                  <ChipList items={k.tags} />
                )}
              </TableCell>
              <TableCell className="text-muted-foreground">{formatTimestamp(k.updatedAt)}</TableCell>
              <TableCell className="text-right">
                <div className="flex items-center justify-end gap-1">
                  <RevealKeyButton id={k.id} reveal={onReveal} />
                  <Button variant="ghost" size="icon" aria-label="Edit" onClick={() => onEdit(k)}>
                    <Pencil className="size-4" />
                  </Button>
                  <ConfirmDelete
                    title={`Delete key "${k.name}"?`}
                    description="Removes the stored public and private key. Endpoints referencing it must be deleted first."
                    onConfirm={() => onDelete(k)}
                  />
                </div>
              </TableCell>
            </TableRow>
          ))}
        </TableBody>
      </Table>
    </div>
  );
}

function RevealKeyButton({
  id,
  reveal,
}: {
  id: bigint;
  reveal: (id: bigint) => Promise<SshKeyValue | undefined>;
}) {
  const [open, setOpen] = React.useState(false);
  const [value, setValue] = React.useState<SshKeyValue | null>(null);
  const [loading, setLoading] = React.useState(false);
  const [showPrivate, setShowPrivate] = React.useState(false);
  const [copied, setCopied] = React.useState<"public" | "private" | null>(null);

  const load = async () => {
    setLoading(true);
    try {
      const v = await reveal(id);
      setValue(v ?? null);
    } catch (err) {
      reportError(err);
    } finally {
      setLoading(false);
    }
  };

  const copy = async (text: string, which: "public" | "private") => {
    try {
      await navigator.clipboard.writeText(text);
      setCopied(which);
      setTimeout(() => setCopied(null), 1500);
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
          setShowPrivate(false);
          setCopied(null);
        }
      }}
    >
      <Button
        variant="ghost"
        size="icon"
        aria-label="Reveal key"
        onClick={async () => {
          setOpen(true);
          await load();
        }}
      >
        <Eye className="size-4" />
      </Button>
      <DialogContent className="sm:max-w-2xl">
        <DialogHeader>
          <DialogTitle>Reveal SSH key</DialogTitle>
          <DialogDescription>
            The private key is fetched on demand. Copy it somewhere safe and close this dialog.
          </DialogDescription>
        </DialogHeader>
        {loading ? (
          <div className="flex items-center gap-2 text-sm text-muted-foreground">
            <Spinner /> Loading…
          </div>
        ) : value === null ? (
          <p className="text-sm text-muted-foreground">No key stored.</p>
        ) : (
          <div className="space-y-3">
            <div className="space-y-1">
              <div className="flex items-center justify-between">
                <Label className="text-xs">Public key</Label>
                <Button
                  variant="ghost"
                  size="sm"
                  className="h-7 gap-1"
                  onClick={() => copy(value.publicKey, "public")}
                >
                  {copied === "public" ? (
                    <Check className="size-3.5 text-emerald-500" />
                  ) : (
                    <Copy className="size-3.5" />
                  )}
                  Copy
                </Button>
              </div>
              <code className="block max-h-40 overflow-auto break-all rounded-md border bg-muted/40 p-3 font-mono text-xs">
                {value.publicKey}
              </code>
            </div>
            <div className="space-y-1">
              <div className="flex items-center justify-between">
                <Label className="text-xs">Private key</Label>
                <div className="flex items-center gap-1">
                  <Button
                    variant="ghost"
                    size="sm"
                    className="h-7 gap-1"
                    onClick={() => setShowPrivate((v) => !v)}
                  >
                    {showPrivate ? <EyeOff className="size-3.5" /> : <Eye className="size-3.5" />}
                    {showPrivate ? "Hide" : "Reveal"}
                  </Button>
                  <Button
                    variant="ghost"
                    size="sm"
                    className="h-7 gap-1"
                    onClick={() => copy(value.privateKey, "private")}
                  >
                    {copied === "private" ? (
                      <Check className="size-3.5 text-emerald-500" />
                    ) : (
                      <Copy className="size-3.5" />
                    )}
                    Copy
                  </Button>
                </div>
              </div>
              <code
                className={cn(
                  "block max-h-48 overflow-auto whitespace-pre-wrap break-all rounded-md border bg-muted/40 p-3 font-mono text-xs",
                  !showPrivate && "select-none blur-sm"
                )}
              >
                {value.privateKey}
              </code>
            </div>
          </div>
        )}
        <DialogFooter>
          <Button variant="outline" onClick={() => setOpen(false)}>
            Close
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}

function KeyDialog({
  mode,
  open,
  onOpenChange,
  keyRow,
  devices,
  deviceById,
  reveal,
  onSubmit,
  onSetValue,
  onSetDevices,
  onSetTags,
}: {
  mode: "create" | "edit";
  open: boolean;
  onOpenChange: (open: boolean) => void;
  keyRow?: SshKeyMetadata | null;
  devices: readonly DeviceMetadata[];
  deviceById: Map<string, DeviceMetadata>;
  reveal?: (id: bigint) => Promise<SshKeyValue | undefined>;
  onSubmit?: (name: string, publicKey: string, privateKey: string, deviceIds: string[], tags: string[]) => Promise<void>;
  onSetValue?: (publicKey: string, privateKey: string) => Promise<void>;
  onSetDevices?: (deviceIds: string[]) => Promise<void>;
  onSetTags?: (tags: string[]) => Promise<void>;
}) {
  const isEdit = mode === "edit";
  const [name, setName] = React.useState("");
  const [publicKey, setPublicKey] = React.useState("");
  const [privateKey, setPrivateKey] = React.useState("");
  const [deviceIds, setDeviceIds] = React.useState<string[]>([]);
  const [tags, setTags] = React.useState<string[]>([]);
  const [busy, setBusy] = React.useState(false);
  const [tab, setTab] = React.useState("key");
  const [revealed, setRevealed] = React.useState<SshKeyValue | null>(null);
  const [showPrivate, setShowPrivate] = React.useState(false);

  React.useEffect(() => {
    if (open) {
      if (isEdit && keyRow) {
        setName(keyRow.name);
        setPublicKey("");
        setPrivateKey("");
        setDeviceIds(keyRow.deviceIds);
        setTags(keyRow.tags);
        setTab("key");
        setRevealed(null);
        setShowPrivate(false);
      } else {
        setName("");
        setPublicKey("");
        setPrivateKey("");
        setDeviceIds([]);
        setTags([]);
        setTab("key");
        setRevealed(null);
        setShowPrivate(false);
      }
    }
  }, [open, isEdit, keyRow]);

  const toggleDevice = (id: string) => {
    setDeviceIds((prev) => (prev.includes(id) ? prev.filter((x) => x !== id) : [...prev, id]));
  };

  const save = async () => {
    if (!name.trim()) {
      reportError(new Error("Name is required"));
      return;
    }
    if (isEdit) {
      setBusy(true);
      try {
        if (publicKey.trim() && privateKey.trim()) {
          await onSetValue?.(publicKey, privateKey);
        }
        await onSetDevices?.(deviceIds);
        await onSetTags?.(tags);
        onOpenChange(false);
        reportSuccess("Key updated.");
      } catch (err) {
        reportError(err);
      } finally {
        setBusy(false);
      }
    } else {
      if (!publicKey.trim() || !privateKey.trim()) {
        reportError(new Error("Both public and private keys are required"));
        return;
      }
      setBusy(true);
      try {
        await onSubmit?.(name, publicKey, privateKey, deviceIds, tags);
        onOpenChange(false);
        reportSuccess("Key created.");
      } catch (err) {
        reportError(err);
      } finally {
        setBusy(false);
      }
    }
  };

  const loadRevealed = async () => {
    if (!reveal || !keyRow) return;
    try {
      const v = await reveal(keyRow.id);
      setRevealed(v ?? null);
    } catch (err) {
      reportError(err);
    }
  };

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="sm:max-w-2xl">
        <DialogHeader>
          <DialogTitle>{isEdit ? `Edit key · ${keyRow?.name ?? ""}` : "New SSH key"}</DialogTitle>
          <DialogDescription>
            {isEdit
              ? "Replace the key material, scope it to devices, and manage tags."
              : "Paste an SSH key pair and scope it to specific devices."}
          </DialogDescription>
        </DialogHeader>

        <div className="space-y-4">
          <div className="space-y-2">
            <Label htmlFor="ssh-key-name">Name</Label>
            <Input
              id="ssh-key-name"
              value={name}
              onChange={(e) => setName(e.target.value)}
              disabled={isEdit}
              placeholder="github-work"
              className="font-mono"
            />
            {isEdit ? (
              <p className="text-[11px] text-muted-foreground">Name is fixed after creation.</p>
            ) : null}
          </div>

          {isEdit ? (
            <Tabs value={tab} onValueChange={setTab}>
              <TabsList className="grid w-full grid-cols-3">
                <TabsTrigger value="key">Key material</TabsTrigger>
                <TabsTrigger value="devices">Devices</TabsTrigger>
                <TabsTrigger value="tags">Tags</TabsTrigger>
              </TabsList>
              <TabsContent value="key" className="mt-3 space-y-3">
                <div className="flex items-center justify-between">
                  <Label className="text-xs">Replace public + private key</Label>
                  <Button variant="ghost" size="sm" className="h-7 gap-1" onClick={loadRevealed}>
                    <Eye className="size-3.5" /> Reveal current
                  </Button>
                </div>
                <Textarea
                  value={publicKey}
                  onChange={(e) => setPublicKey(e.target.value)}
                  placeholder="ssh-ed25519 AAAA… user@host"
                  className="h-32 font-mono text-xs"
                />
                <Textarea
                  value={privateKey}
                  onChange={(e) => setPrivateKey(e.target.value)}
                  placeholder="-----BEGIN OPENSSH PRIVATE KEY-----"
                  className="h-40 font-mono text-xs"
                />
                <p className="text-[11px] text-muted-foreground">
                  Leave both fields blank to keep the existing key material.
                </p>
                {revealed ? (
                  <div className="space-y-1 rounded-md border bg-muted/30 p-2">
                    <div className="flex items-center justify-between">
                      <span className="text-[11px] text-muted-foreground">Current public key</span>
                      <span className="font-mono text-[10px] text-muted-foreground">{revealed.fingerprint}</span>
                    </div>
                    <code className="block break-all font-mono text-[11px]">{revealed.publicKey}</code>
                    <div className="mt-1 flex items-center justify-between">
                      <span className="text-[11px] text-muted-foreground">Current private key</span>
                      <Button
                        variant="ghost"
                        size="sm"
                        className="h-6 gap-1"
                        onClick={() => setShowPrivate((v) => !v)}
                      >
                        {showPrivate ? <EyeOff className="size-3" /> : <Eye className="size-3" />}
                        {showPrivate ? "Hide" : "Reveal"}
                      </Button>
                    </div>
                    <code
                      className={cn(
                        "block max-h-32 overflow-auto whitespace-pre-wrap break-all font-mono text-[11px]",
                        !showPrivate && "select-none blur-sm"
                      )}
                    >
                      {revealed.privateKey}
                    </code>
                  </div>
                ) : null}
              </TabsContent>
              <TabsContent value="devices" className="mt-3 space-y-2">
                <div className="space-y-1">
                  <Label>Device scope</Label>
                  <p className="text-xs text-muted-foreground">
                    Leave empty to allow every device to use this SSH key. Select devices to restrict it.
                  </p>
                </div>
                <DevicePicker devices={devices} selected={deviceIds} onToggle={toggleDevice} deviceById={deviceById} />
              </TabsContent>
              <TabsContent value="tags" className="mt-3 space-y-2">
                <div className="space-y-1">
                  <Label>Organization tags</Label>
                  <p className="text-xs text-muted-foreground">Tags are labels for grouping and filtering. They do not grant access.</p>
                </div>
                <TagInput values={tags} onChange={setTags} pattern={TAG_PATTERN} emptyLabel="No tags added." />
              </TabsContent>
            </Tabs>
          ) : (
            <div className="space-y-4">
              <div className="space-y-2">
                <Label htmlFor="ssh-pub">Public key</Label>
                <Textarea
                  id="ssh-pub"
                  value={publicKey}
                  onChange={(e) => setPublicKey(e.target.value)}
                  placeholder="ssh-ed25519 AAAA… user@host"
                  className="h-32 font-mono text-xs"
                />
              </div>
              <div className="space-y-2">
                <Label htmlFor="ssh-priv">Private key</Label>
                <Textarea
                  id="ssh-priv"
                  value={privateKey}
                  onChange={(e) => setPrivateKey(e.target.value)}
                  placeholder="-----BEGIN OPENSSH PRIVATE KEY-----"
                  className="h-40 font-mono text-xs"
                />
              </div>
              <Separator />
              <div className="space-y-2">
                <div className="space-y-1">
                  <Label>Device scope</Label>
                  <p className="text-xs text-muted-foreground">
                    Leave empty to allow every device to use this SSH key. Select devices to restrict it.
                  </p>
                </div>
                <DevicePicker devices={devices} selected={deviceIds} onToggle={toggleDevice} deviceById={deviceById} />
              </div>
              <div className="space-y-2">
                <div className="space-y-1">
                  <Label>Organization tags</Label>
                  <p className="text-xs text-muted-foreground">Tags are labels for grouping and filtering. They do not grant access.</p>
                </div>
                <TagInput values={tags} onChange={setTags} pattern={TAG_PATTERN} emptyLabel="No tags added." />
              </div>
            </div>
          )}
        </div>

        <DialogFooter>
          <Button variant="outline" onClick={() => onOpenChange(false)} disabled={busy}>
            Cancel
          </Button>
          <Button onClick={save} disabled={busy || !name.trim()}>
            {busy ? <Spinner /> : null}
            {isEdit ? "Save" : "Create key"}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}

function EndpointsSection({
  ready,
  endpoints,
  keysById,
  deviceById,
  relayDeviceId,
  onCreate,
  onEdit,
  onConnect,
  onToggleEnabled,
  onDelete,
}: {
  ready: boolean;
  endpoints: SshEndpointMetadata[];
  keysById: Map<string, SshKeyMetadata>;
  deviceById: Map<string, DeviceMetadata>;
  relayDeviceId: bigint | undefined;
  onCreate: () => void;
  onEdit: (e: SshEndpointMetadata) => void;
  onConnect: (e: SshEndpointMetadata) => void;
  onToggleEnabled: (e: SshEndpointMetadata, enabled: boolean) => Promise<void>;
  onDelete: (e: SshEndpointMetadata) => Promise<void>;
}) {
  if (!ready) {
    return (
      <div className="flex justify-center p-10 text-muted-foreground">
        <Spinner className="size-5" />
      </div>
    );
  }
  if (endpoints.length === 0) {
    return (
      <EmptyState
        icon={Server}
        title="No SSH endpoints yet"
        description="Define a host, port, username, and the SSH key it should use. Toggle SSH for the devices that should receive it."
        action={
          <Button onClick={onCreate} className="gap-2">
            <Plus className="size-4" /> New endpoint
          </Button>
        }
      />
    );
  }
  return (
    <div className="rounded-lg border">
      <Table>
        <TableHeader>
          <TableRow>
            <TableHead className="w-16">SSH</TableHead>
            <TableHead>Name</TableHead>
            <TableHead>Host:port</TableHead>
            <TableHead>User</TableHead>
            <TableHead>Key</TableHead>
            <TableHead>Devices</TableHead>
            <TableHead>Tags</TableHead>
            <TableHead>Updated</TableHead>
            <TableHead className="w-32 text-right">Actions</TableHead>
          </TableRow>
        </TableHeader>
        <TableBody>
          {endpoints.map((e) => {
            const key = keysById.get(String(e.keyId));
            return (
              <TableRow key={String(e.id)} className={cn(!e.enabled && "opacity-60")}>
                <TableCell>
                  <Switch
                    checked={e.enabled}
                    onCheckedChange={(v) => onToggleEnabled(e, Boolean(v))}
                    aria-label={e.enabled ? "Disable ssh" : "Enable ssh"}
                  />
                </TableCell>
                <TableCell>
                  <div className="flex items-center gap-2">
                    <span className="font-mono font-medium">{e.name}</span>
                    {e.enabled ? (
                      <Badge variant="success" className="gap-1">
                        <ShieldCheck className="size-3" /> on
                      </Badge>
                    ) : (
                      <Badge variant="secondary" className="gap-1">
                        <ShieldOff className="size-3" /> off
                      </Badge>
                    )}
                  </div>
                </TableCell>
                <TableCell>
                  <span className="inline-flex items-center gap-1 font-mono text-xs">
                    <Network className="size-3 text-muted-foreground" />
                    {e.host}:{e.port}
                  </span>
                </TableCell>
                <TableCell>
                  <span className="inline-flex items-center gap-1 font-mono text-xs">
                    <Terminal className="size-3 text-muted-foreground" />
                    {e.username}
                  </span>
                </TableCell>
                <TableCell>
                  {key ? (
                    <span className="inline-flex items-center gap-1 font-mono text-xs">
                      <KeyRound className="size-3 text-muted-foreground" />
                      {key.name}
                    </span>
                  ) : (
                    <span className="text-destructive">missing #{String(e.keyId)}</span>
                  )}
                </TableCell>
                <TableCell>
                  {e.deviceIds.length === 0 ? (
                    <span className="text-muted-foreground">all</span>
                  ) : (
                    <DeviceChips ids={e.deviceIds} deviceById={deviceById} />
                  )}
                </TableCell>
                <TableCell>
                  {e.tags.length === 0 ? (
                    <span className="text-muted-foreground">—</span>
                  ) : (
                    <ChipList items={e.tags} />
                  )}
                </TableCell>
                <TableCell className="text-muted-foreground">{formatTimestamp(e.updatedAt)}</TableCell>
                <TableCell className="text-right">
                  <div className="flex items-center justify-end gap-1">
                    <Button
                      variant="ghost"
                      size="icon"
                      aria-label="Open in browser terminal"
                      title={
                        relayDeviceId
                          ? "Open in browser terminal"
                          : "Pick a relay device on the Devices page first"
                      }
                      disabled={!e.enabled || !relayDeviceId}
                      onClick={() => onConnect(e)}
                    >
                      <Play className="size-4" />
                    </Button>
                    <Button variant="ghost" size="icon" aria-label="Edit" onClick={() => onEdit(e)}>
                      <Pencil className="size-4" />
                    </Button>
                    <ConfirmDelete
                      title={`Delete endpoint "${e.name}"?`}
                      description="This removes the endpoint configuration. The underlying SSH key is not affected."
                      onConfirm={() => onDelete(e)}
                    />
                  </div>
                </TableCell>
              </TableRow>
            );
          })}
        </TableBody>
      </Table>
    </div>
  );
}

function EndpointDialog({
  mode,
  open,
  onOpenChange,
  endpoint,
  keys,
  devices,
  deviceById,
  onSubmit,
  onUpdate,
  onSetDevices,
  onSetTags,
}: {
  mode: "create" | "edit";
  open: boolean;
  onOpenChange: (open: boolean) => void;
  endpoint?: SshEndpointMetadata | null;
  keys: SshKeyMetadata[];
  devices: readonly DeviceMetadata[];
  deviceById: Map<string, DeviceMetadata>;
  onSubmit?: (
    name: string,
    host: string,
    port: number,
    username: string,
    keyId: bigint,
    deviceIds: string[],
    tags: string[],
    enabled: boolean,
  ) => Promise<void>;
  onUpdate?: (host: string, port: number, username: string, keyId: bigint) => Promise<void>;
  onSetDevices?: (deviceIds: string[]) => Promise<void>;
  onSetTags?: (tags: string[]) => Promise<void>;
}) {
  const isEdit = mode === "edit";
  const [name, setName] = React.useState("");
  const [host, setHost] = React.useState("");
  const [port, setPort] = React.useState("22");
  const [username, setUsername] = React.useState("");
  const [keyId, setKeyId] = React.useState<string>("");
  const [deviceIds, setDeviceIds] = React.useState<string[]>([]);
  const [tags, setTags] = React.useState<string[]>([]);
  const [enabled, setEnabled] = React.useState(true);
  const [busy, setBusy] = React.useState(false);
  const [tab, setTab] = React.useState("connection");

  React.useEffect(() => {
    if (open) {
      if (isEdit && endpoint) {
        setName(endpoint.name);
        setHost(endpoint.host);
        setPort(String(endpoint.port));
        setUsername(endpoint.username);
        setKeyId(String(endpoint.keyId));
        setDeviceIds(endpoint.deviceIds);
        setTags(endpoint.tags);
        setEnabled(endpoint.enabled);
        setTab("connection");
      } else {
        setName("");
        setHost("");
        setPort("22");
        setUsername("");
        setKeyId(keys[0] ? String(keys[0].id) : "");
        setDeviceIds([]);
        setTags([]);
        setEnabled(true);
        setTab("connection");
      }
    }
  }, [open, isEdit, endpoint, keys]);

  const toggleDevice = (id: string) => {
    setDeviceIds((prev) => (prev.includes(id) ? prev.filter((x) => x !== id) : [...prev, id]));
  };

  const portNum = Number.parseInt(port, 10);

  const save = async () => {
    if (!name.trim()) {
      reportError(new Error("Name is required"));
      return;
    }
    if (!host.trim()) {
      reportError(new Error("Host is required"));
      return;
    }
    if (!Number.isFinite(portNum) || portNum < 1 || portNum > 65535) {
      reportError(new Error("Port must be 1..=65535"));
      return;
    }
    if (!username.trim()) {
      reportError(new Error("Username is required"));
      return;
    }
    if (!keyId) {
      reportError(new Error("Pick an SSH key"));
      return;
    }
    if (isEdit) {
      setBusy(true);
      try {
        await onUpdate?.(host, portNum, username, BigInt(keyId));
        await onSetDevices?.(deviceIds);
        await onSetTags?.(tags);
        onOpenChange(false);
        reportSuccess("Endpoint updated.");
      } catch (err) {
        reportError(err);
      } finally {
        setBusy(false);
      }
    } else {
      setBusy(true);
      try {
        await onSubmit?.(name, host, portNum, username, BigInt(keyId), deviceIds, tags, enabled);
        onOpenChange(false);
        reportSuccess("Endpoint created.");
      } catch (err) {
        reportError(err);
      } finally {
        setBusy(false);
      }
    }
  };

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="sm:max-w-2xl">
        <DialogHeader>
          <DialogTitle>{isEdit ? `Edit endpoint · ${endpoint?.name ?? ""}` : "New SSH endpoint"}</DialogTitle>
          <DialogDescription>
            {isEdit
              ? "Update connection details, devices, and tags."
              : "Define a host, the user to connect as, and which key to authenticate with."}
          </DialogDescription>
        </DialogHeader>

        <div className="space-y-4">
          <div className="space-y-2">
            <Label htmlFor="ep-name">Name</Label>
            <Input
              id="ep-name"
              value={name}
              onChange={(e) => setName(e.target.value)}
              disabled={isEdit}
              placeholder="prod-bastion"
              className="font-mono"
            />
            {isEdit ? (
              <p className="text-[11px] text-muted-foreground">Name is fixed after creation.</p>
            ) : null}
          </div>

          {isEdit ? (
            <Tabs value={tab} onValueChange={setTab}>
              <TabsList className="grid w-full grid-cols-3">
                <TabsTrigger value="connection">Connection</TabsTrigger>
                <TabsTrigger value="devices">Devices</TabsTrigger>
                <TabsTrigger value="tags">Tags</TabsTrigger>
              </TabsList>
              <TabsContent value="connection" className="mt-3 grid gap-3 sm:grid-cols-2">
                <div className="space-y-2 sm:col-span-2">
                  <Label htmlFor="ep-host">Host</Label>
                  <Input id="ep-host" value={host} onChange={(e) => setHost(e.target.value)} className="font-mono" />
                </div>
                <div className="space-y-2">
                  <Label htmlFor="ep-port">Port</Label>
                  <Input
                    id="ep-port"
                    type="number"
                    value={port}
                    onChange={(e) => setPort(e.target.value)}
                    min={1}
                    max={65535}
                    className="font-mono"
                  />
                </div>
                <div className="space-y-2">
                  <Label htmlFor="ep-user">Username</Label>
                  <Input
                    id="ep-user"
                    value={username}
                    onChange={(e) => setUsername(e.target.value)}
                    className="font-mono"
                  />
                </div>
                <div className="space-y-2 sm:col-span-2">
                  <Label>SSH key</Label>
                  <Select value={keyId} onValueChange={setKeyId}>
                    <SelectTrigger>
                      <SelectValue placeholder="Select a key" />
                    </SelectTrigger>
                    <SelectContent>
                      {keys.map((k) => (
                        <SelectItem key={String(k.id)} value={String(k.id)}>
                          {k.name}
                        </SelectItem>
                      ))}
                    </SelectContent>
                  </Select>
                </div>
              </TabsContent>
              <TabsContent value="devices" className="mt-3 space-y-2">
                <div className="space-y-1">
                  <Label>Device scope</Label>
                  <p className="text-xs text-muted-foreground">
                    Leave empty to show this endpoint to all devices. Select devices to restrict visibility.
                  </p>
                </div>
                <DevicePicker devices={devices} selected={deviceIds} onToggle={toggleDevice} deviceById={deviceById} />
              </TabsContent>
              <TabsContent value="tags" className="mt-3 space-y-2">
                <div className="space-y-1">
                  <Label>Organization tags</Label>
                  <p className="text-xs text-muted-foreground">Tags are labels for grouping and filtering. They do not grant access.</p>
                </div>
                <TagInput values={tags} onChange={setTags} pattern={TAG_PATTERN} emptyLabel="No tags added." />
              </TabsContent>
            </Tabs>
          ) : (
            <div className="space-y-4">
              <div className="grid gap-3 sm:grid-cols-2">
                <div className="space-y-2 sm:col-span-2">
                  <Label htmlFor="ep-host-new">Host</Label>
                  <Input
                    id="ep-host-new"
                    value={host}
                    onChange={(e) => setHost(e.target.value)}
                    placeholder="bastion.example.com"
                    className="font-mono"
                  />
                </div>
                <div className="space-y-2">
                  <Label htmlFor="ep-port-new">Port</Label>
                  <Input
                    id="ep-port-new"
                    type="number"
                    value={port}
                    onChange={(e) => setPort(e.target.value)}
                    min={1}
                    max={65535}
                    className="font-mono"
                  />
                </div>
                <div className="space-y-2">
                  <Label htmlFor="ep-user-new">Username</Label>
                  <Input
                    id="ep-user-new"
                    value={username}
                    onChange={(e) => setUsername(e.target.value)}
                    placeholder="deploy"
                    className="font-mono"
                  />
                </div>
                <div className="space-y-2 sm:col-span-2">
                  <Label>SSH key</Label>
                  <Select value={keyId} onValueChange={setKeyId}>
                    <SelectTrigger>
                      <SelectValue placeholder="Select a key" />
                    </SelectTrigger>
                    <SelectContent>
                      {keys.map((k) => (
                        <SelectItem key={String(k.id)} value={String(k.id)}>
                          {k.name}
                        </SelectItem>
                      ))}
                    </SelectContent>
                  </Select>
                </div>
                <div className="flex items-center gap-2 sm:col-span-2">
                  <Switch
                    id="ep-enabled"
                    checked={enabled}
                    onCheckedChange={setEnabled}
                  />
                  <Label htmlFor="ep-enabled">Enabled by default</Label>
                </div>
              </div>
              <Separator />
              <div className="space-y-2">
                <div className="space-y-1">
                  <Label>Device scope</Label>
                  <p className="text-xs text-muted-foreground">
                    Leave empty to show this endpoint to all devices. Select devices to restrict visibility.
                  </p>
                </div>
                <DevicePicker devices={devices} selected={deviceIds} onToggle={toggleDevice} deviceById={deviceById} />
              </div>
              <div className="space-y-2">
                <div className="space-y-1">
                  <Label>Organization tags</Label>
                  <p className="text-xs text-muted-foreground">Tags are labels for grouping and filtering. They do not grant access.</p>
                </div>
                <TagInput values={tags} onChange={setTags} pattern={TAG_PATTERN} emptyLabel="No tags added." />
              </div>
            </div>
          )}
        </div>

        <DialogFooter>
          <Button variant="outline" onClick={() => onOpenChange(false)} disabled={busy}>
            Cancel
          </Button>
          <Button onClick={save} disabled={busy || !name.trim()}>
            {busy ? <Spinner /> : null}
            {isEdit ? "Save" : "Create endpoint"}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
