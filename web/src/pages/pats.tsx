import * as React from "react";
import { useProcedure, useReducer, useTable } from "spacetimedb/react";
import { KeyRound, Plus, Copy, Check, ShieldAlert, ShieldCheck, Pencil } from "lucide-react";

import { procedures, reducers, tables } from "@/module_bindings";
import type { ApiKeyMetadata, CreatedApiKey } from "@/module_bindings/types";
import { unwrap } from "@/lib/stdb";
import { formatTimestamp, shortId } from "@/lib/utils";
import { reportError, reportSuccess } from "@/lib/toast";
import { PageHeader, EmptyState, ConfirmDelete, Spinner, ChipList } from "@/components/common";
import { TagInput } from "@/components/tag-input";
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
} from "@/components/ui/dialog";
import {
  AlertDialog,
  AlertDialogAction,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
  AlertDialogTrigger,
} from "@/components/ui/alert-dialog";

const PERMISSION_PATTERN = /^[A-Za-z0-9:._\-*]+$/;

export function PatsPage() {
  const [rows, ready] = useTable(tables.my_api_keys);
  const createProc = useProcedure(procedures.createApiKey);
  const revoke = useReducer(reducers.revokeApiKey);
  const updatePerms = useReducer(reducers.updateApiKeyPermissions);

  const [createOpen, setCreateOpen] = React.useState(false);
  const [created, setCreated] = React.useState<CreatedApiKey | null>(null);
  const [editing, setEditing] = React.useState<ApiKeyMetadata | null>(null);

  const keys = React.useMemo(
    () =>
      [...rows].sort((a, b) => Number(b.createdAt.microsSinceUnixEpoch - a.createdAt.microsSinceUnixEpoch)),
    [rows]
  );

  return (
    <div>
      <PageHeader
        title="Personal Access Tokens"
        description="PATs authenticate the SpaceNix CLI, TUI, and devices. Scope each token with permission grants like files:read or secrets:*."
        actions={
          <Button onClick={() => setCreateOpen(true)} className="gap-2">
            <Plus className="size-4" /> New token
          </Button>
        }
      />

      <CreatePatDialog
        open={createOpen}
        onOpenChange={setCreateOpen}
        onCreate={async (name, permissions) => {
          const res = await createProc({ name, permissions });
          const created = unwrap<CreatedApiKey>(res);
          setCreated(created);
        }}
      />

      <TokenRevealDialog
        created={created}
        onOpenChange={(o) => !o && setCreated(null)}
      />

      <EditPermissionsDialog
        key={editing ? String(editing.id) : "none"}
        apikey={editing}
        onOpenChange={(o) => !o && setEditing(null)}
        onSave={async (permissions) => {
          if (!editing) return;
          await updatePerms({ apiKeyId: editing.id, permissions });
        }}
      />

      {!ready ? (
        <div className="flex justify-center p-10 text-muted-foreground">
          <Spinner className="size-5" />
        </div>
      ) : keys.length === 0 ? (
        <EmptyState
          icon={KeyRound}
          title="No access tokens"
          description="Create a PAT to grant scoped access from your devices or automation. Tokens are shown once at creation."
          action={
            <Button onClick={() => setCreateOpen(true)} className="gap-2">
              <Plus className="size-4" /> New token
            </Button>
          }
        />
      ) : (
        <div className="rounded-lg border">
          <Table>
            <TableHeader>
              <TableRow>
                <TableHead>Name</TableHead>
                <TableHead>Permissions</TableHead>
                <TableHead>Status</TableHead>
                <TableHead>Created</TableHead>
                <TableHead>Last used</TableHead>
                <TableHead className="w-32 text-right">Actions</TableHead>
              </TableRow>
            </TableHeader>
            <TableBody>
              {keys.map((k) => {
                const revoked = Boolean(k.revokedAt);
                return (
                  <TableRow key={String(k.id)} className={revoked ? "opacity-60" : undefined}>
                    <TableCell className="font-medium">
                      {k.name}
                      <div className="font-mono text-[10px] text-muted-foreground">#{shortId(String(k.id), 4, 4)}</div>
                    </TableCell>
                    <TableCell>
                      <ChipList items={k.permissions} />
                    </TableCell>
                    <TableCell>
                      {revoked ? (
                        <Badge variant="destructive" className="gap-1">
                          <ShieldAlert className="size-3" /> revoked
                        </Badge>
                      ) : (
                        <Badge variant="success" className="gap-1">
                          <ShieldCheck className="size-3" /> active
                        </Badge>
                      )}
                    </TableCell>
                    <TableCell className="text-muted-foreground">{formatTimestamp(k.createdAt)}</TableCell>
                    <TableCell className="text-muted-foreground">
                      {k.lastUsedAt ? formatTimestamp(k.lastUsedAt) : "never"}
                    </TableCell>
                    <TableCell className="text-right">
                      <div className="flex items-center justify-end gap-1">
                        <Button
                          variant="ghost"
                          size="icon"
                          aria-label="Edit permissions"
                          disabled={revoked}
                          onClick={() => setEditing(k)}
                        >
                          <Pencil className="size-4" />
                        </Button>
                        {revoked ? null : (
                          <AlertDialog>
                            <AlertDialogTrigger asChild>
                              <Button variant="ghost" size="sm" className="text-destructive">
                                Revoke
                              </Button>
                            </AlertDialogTrigger>
                            <AlertDialogContent>
                              <AlertDialogHeader>
                                <AlertDialogTitle>Revoke "{k.name}"?</AlertDialogTitle>
                                <AlertDialogDescription>
                                  The token stops working immediately. This cannot be undone.
                                </AlertDialogDescription>
                              </AlertDialogHeader>
                              <AlertDialogFooter>
                                <AlertDialogCancel>Cancel</AlertDialogCancel>
                                <AlertDialogAction
                                  className="bg-destructive text-destructive-foreground hover:bg-destructive/90"
                                  onClick={async () => {
                                    try {
                                      await revoke({ apiKeyId: k.id });
                                      reportSuccess("Token revoked.");
                                    } catch (err) {
                                      reportError(err);
                                    }
                                  }}
                                >
                                  Revoke
                                </AlertDialogAction>
                              </AlertDialogFooter>
                            </AlertDialogContent>
                          </AlertDialog>
                        )}
                      </div>
                    </TableCell>
                  </TableRow>
                );
              })}
            </TableBody>
          </Table>
        </div>
      )}
    </div>
  );
}

function CreatePatDialog({
  open,
  onOpenChange,
  onCreate,
}: {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  onCreate: (name: string, permissions: string[]) => Promise<void>;
}) {
  const [name, setName] = React.useState("");
  const [permissions, setPermissions] = React.useState<string[]>([]);
  const [busy, setBusy] = React.useState(false);

  React.useEffect(() => {
    if (open) {
      setName("");
      setPermissions([]);
    }
  }, [open]);

  const submit = async () => {
    if (!name.trim()) return;
    if (permissions.length === 0) {
      reportError(new Error("At least one permission is required"));
      return;
    }
    setBusy(true);
    try {
      await onCreate(name.trim(), permissions);
      onOpenChange(false);
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
          <DialogTitle>New access token</DialogTitle>
          <DialogDescription>Choose a label and the permission grants this token will carry.</DialogDescription>
        </DialogHeader>
        <div className="space-y-4">
          <div className="space-y-2">
            <Label htmlFor="pat-name">Name</Label>
            <Input id="pat-name" placeholder="laptop-cli" value={name} onChange={(e) => setName(e.target.value)} maxLength={128} autoFocus />
          </div>
          <div className="space-y-2">
            <Label>Permissions</Label>
            <TagInput values={permissions} onChange={setPermissions} pattern={PERMISSION_PATTERN} placeholder="files:read" />
          </div>
        </div>
        <DialogFooter>
          <Button variant="outline" onClick={() => onOpenChange(false)} disabled={busy}>
            Cancel
          </Button>
          <Button onClick={submit} disabled={busy || !name.trim()}>
            {busy ? <Spinner /> : null}
            Generate token
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}

function TokenRevealDialog({
  created,
  onOpenChange,
}: {
  created: CreatedApiKey | null;
  onOpenChange: (open: boolean) => void;
}) {
  const [copied, setCopied] = React.useState(false);
  const token = created?.token ?? "";

  const copy = async () => {
    try {
      await navigator.clipboard.writeText(token);
      setCopied(true);
      setTimeout(() => setCopied(false), 2000);
    } catch {
      reportError(new Error("Clipboard unavailable"));
    }
  };

  return (
    <Dialog open={created !== null} onOpenChange={onOpenChange}>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>Token created</DialogTitle>
          <DialogDescription>
            Copy this token now. For security, it will not be shown again.
          </DialogDescription>
        </DialogHeader>
        <div className="space-y-3">
          <div className="flex items-start gap-2 rounded-md border bg-amber-500/10 p-3 text-sm text-amber-700 dark:text-amber-400">
            <ShieldAlert className="mt-0.5 size-4 shrink-0" />
            <span>Treat this token like a password. Anyone with it can act on your account within its granted permissions.</span>
          </div>
          <div className="flex items-start gap-2">
            <code className="block flex-1 break-all rounded-md border bg-muted/40 p-3 font-mono text-sm">{token}</code>
            <Button variant="outline" size="icon" onClick={copy} aria-label="Copy token">
              {copied ? <Check className="size-4 text-emerald-500" /> : <Copy className="size-4" />}
            </Button>
          </div>
          {created ? (
            <div className="text-xs text-muted-foreground">
              Name: <span className="font-medium text-foreground">{created.metadata.name}</span> · id #
              {shortId(String(created.metadata.id), 4, 4)}
            </div>
          ) : null}
        </div>
        <DialogFooter>
          <Button onClick={() => onOpenChange(false)}>Done</Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}

function EditPermissionsDialog({
  apikey,
  onOpenChange,
  onSave,
}: {
  apikey: ApiKeyMetadata | null;
  onOpenChange: (open: boolean) => void;
  onSave: (permissions: string[]) => Promise<void>;
}) {
  const [permissions, setPermissions] = React.useState<string[]>([]);
  const [busy, setBusy] = React.useState(false);

  React.useEffect(() => {
    if (apikey) setPermissions(apikey.permissions);
  }, [apikey]);

  const save = async () => {
    if (!apikey) return;
    if (permissions.length === 0) {
      reportError(new Error("At least one permission is required"));
      return;
    }
    setBusy(true);
    try {
      await onSave(permissions);
      onOpenChange(false);
      reportSuccess("Permissions updated.");
    } catch (err) {
      reportError(err);
    } finally {
      setBusy(false);
    }
  };

  return (
    <Dialog open={apikey !== null} onOpenChange={onOpenChange}>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>Edit permissions · {apikey?.name ?? ""}</DialogTitle>
          <DialogDescription>Replace the permission grants carried by this token.</DialogDescription>
        </DialogHeader>
        <div className="space-y-2">
          <Label>Permissions</Label>
          <TagInput values={permissions} onChange={setPermissions} pattern={PERMISSION_PATTERN} />
        </div>
        <Separator />
        <DialogFooter>
          <Button variant="outline" onClick={() => onOpenChange(false)} disabled={busy}>
            Cancel
          </Button>
          <Button onClick={save} disabled={busy}>
            {busy ? <Spinner /> : null}
            Save permissions
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
