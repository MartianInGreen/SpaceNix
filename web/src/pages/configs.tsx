import * as React from "react";
import { useReducer, useTable } from "spacetimedb/react";
import { FileCode, Plus, Pencil, Save, X } from "lucide-react";

import { reducers, tables } from "@/module_bindings";
import type { UserConfigMetadata } from "@/module_bindings/types";
import { formatTimestamp } from "@/lib/utils";
import { reportError, reportSuccess } from "@/lib/toast";
import { PageHeader, EmptyState, ConfirmDelete, Spinner } from "@/components/common";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Textarea } from "@/components/ui/textarea";
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

const NAME_PATTERN = /^[A-Za-z0-9._\-/]+$/;

export function ConfigsPage() {
  const [rows, ready] = useTable(tables.my_configs);
  const setConfig = useReducer(reducers.setConfig);
  const deleteConfig = useReducer(reducers.deleteConfig);

  const [createOpen, setCreateOpen] = React.useState(false);
  const [editing, setEditing] = React.useState<UserConfigMetadata | null>(null);

  const configs = React.useMemo(
    () => [...rows].sort((a, b) => a.name.localeCompare(b.name)),
    [rows]
  );

  return (
    <div>
      <PageHeader
        title="Configs"
        description="Plain-text configuration files synced to your devices. Edit them inline."
        actions={
          <Button onClick={() => setCreateOpen(true)} className="gap-2">
            <Plus className="size-4" /> New config
          </Button>
        }
      />

      <CreateConfigDialog
        open={createOpen}
        onOpenChange={setCreateOpen}
        onCreate={async (name, content) => {
          await setConfig({ name, content });
        }}
      />

      <EditConfigDialog
        config={editing}
        onOpenChange={(o) => !o && setEditing(null)}
        onSave={async (name, content) => {
          await setConfig({ name, content });
        }}
      />

      {!ready ? (
        <div className="flex justify-center p-10 text-muted-foreground">
          <Spinner className="size-5" />
        </div>
      ) : configs.length === 0 ? (
        <EmptyState
          icon={FileCode}
          title="No configs yet"
          description="Create a config file (e.g. shell/rc, app.toml) and edit it inline. Configs are versioned by name and synced to your devices."
          action={
            <Button onClick={() => setCreateOpen(true)} className="gap-2">
              <Plus className="size-4" /> New config
            </Button>
          }
        />
      ) : (
        <div className="rounded-lg border">
          <Table>
            <TableHeader>
              <TableRow>
                <TableHead>Name</TableHead>
                <TableHead>Size</TableHead>
                <TableHead>Updated</TableHead>
                <TableHead className="w-32 text-right">Actions</TableHead>
              </TableRow>
            </TableHeader>
            <TableBody>
              {configs.map((c) => (
                <TableRow key={String(c.id)} className="cursor-pointer" onClick={() => setEditing(c)}>
                  <TableCell className="font-mono font-medium">{c.name}</TableCell>
                  <TableCell className="text-muted-foreground">
                    {new Blob([c.content]).size} B
                  </TableCell>
                  <TableCell className="text-muted-foreground">{formatTimestamp(c.updatedAt)}</TableCell>
                  <TableCell className="text-right" onClick={(e) => e.stopPropagation()}>
                    <div className="flex items-center justify-end gap-1">
                      <Button variant="ghost" size="icon" aria-label="Edit" onClick={() => setEditing(c)}>
                        <Pencil className="size-4" />
                      </Button>
                      <ConfirmDelete
                        title={`Delete config "${c.name}"?`}
                        description="This removes the config from syncing. Devices will no longer receive it."
                        onConfirm={async () => {
                          await deleteConfig({ configId: c.id });
                          reportSuccess("Config deleted.");
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

function CreateConfigDialog({
  open,
  onOpenChange,
  onCreate,
}: {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  onCreate: (name: string, content: string) => Promise<void>;
}) {
  const [name, setName] = React.useState("");
  const [content, setContent] = React.useState("");
  const [busy, setBusy] = React.useState(false);

  React.useEffect(() => {
    if (open) {
      setName("");
      setContent("");
    }
  }, [open]);

  const submit = async () => {
    if (!NAME_PATTERN.test(name)) {
      reportError(new Error("Name may only contain [A-Za-z0-9._-/]"));
      return;
    }
    setBusy(true);
    try {
      await onCreate(name, content);
      onOpenChange(false);
      reportSuccess("Config created.");
    } catch (err) {
      reportError(err);
    } finally {
      setBusy(false);
    }
  };

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="sm:max-w-2xl">
        <DialogHeader>
          <DialogTitle>New config</DialogTitle>
          <DialogDescription>Give it a path-like name, then add the content.</DialogDescription>
        </DialogHeader>
        <div className="space-y-4">
          <div className="space-y-2">
            <Label htmlFor="cfg-name">Name</Label>
            <Input id="cfg-name" placeholder="shell/zshrc" value={name} onChange={(e) => setName(e.target.value)} className="font-mono" autoFocus />
          </div>
          <div className="space-y-2">
            <Label htmlFor="cfg-content">Content</Label>
            <Textarea
              id="cfg-content"
              value={content}
              onChange={(e) => setContent(e.target.value)}
              className="min-h-[240px] font-mono text-sm"
              placeholder="# add your config here"
            />
          </div>
        </div>
        <DialogFooter>
          <Button variant="outline" onClick={() => onOpenChange(false)} disabled={busy}>
            Cancel
          </Button>
          <Button onClick={submit} disabled={busy || !name}>
            {busy ? <Spinner /> : null}
            Create
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}

function EditConfigDialog({
  config,
  onOpenChange,
  onSave,
}: {
  config: UserConfigMetadata | null;
  onOpenChange: (open: boolean) => void;
  onSave: (name: string, content: string) => Promise<void>;
}) {
  const [content, setContent] = React.useState("");
  const [dirty, setDirty] = React.useState(false);
  const [busy, setBusy] = React.useState(false);

  React.useEffect(() => {
    if (config) {
      setContent(config.content);
      setDirty(false);
    }
  }, [config]);

  const save = async () => {
    if (!config) return;
    setBusy(true);
    try {
      await onSave(config.name, content);
      setDirty(false);
      onOpenChange(false);
      reportSuccess("Config saved.");
    } catch (err) {
      reportError(err);
    } finally {
      setBusy(false);
    }
  };

  return (
    <Dialog open={config !== null} onOpenChange={onOpenChange}>
      <DialogContent className="sm:max-w-3xl">
        <DialogHeader>
          <DialogTitle className="flex items-center gap-2 font-mono">
            <FileCode className="size-4" />
            {config?.name ?? ""}
          </DialogTitle>
          <DialogDescription>
            {config ? `Updated ${formatTimestamp(config.updatedAt)}` : ""}
          </DialogDescription>
        </DialogHeader>
        <div className="space-y-2">
          <div className="flex items-center justify-between">
            <Badge variant={dirty ? "warning" : "secondary"}>
              {dirty ? "unsaved changes" : "saved"}
            </Badge>
            <span className="text-xs text-muted-foreground">{new Blob([content]).size} B</span>
          </div>
          <Textarea
            value={content}
            onChange={(e) => {
              setContent(e.target.value);
              setDirty(true);
            }}
            className="min-h-[420px] font-mono text-sm leading-relaxed"
            spellCheck={false}
          />
        </div>
        <Separator />
        <DialogFooter>
          <Button
            variant="outline"
            onClick={() => {
              if (dirty) {
                setContent(config?.content ?? "");
                setDirty(false);
              }
              onOpenChange(false);
            }}
            disabled={busy}
          >
            <X className="size-4" /> Cancel
          </Button>
          <Button onClick={save} disabled={busy || !dirty}>
            {busy ? <Spinner /> : <Save className="size-4" />}
            Save
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
