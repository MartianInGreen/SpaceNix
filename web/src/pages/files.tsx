import * as React from "react";
import { useProcedure, useReducer, useTable } from "spacetimedb/react";
import {
  Cloud,
  Download,
  FilesIcon,
  Pencil,
  Plus,
  Save,
  Settings,
  Upload,
  X,
} from "lucide-react";

import { procedures, reducers, tables } from "@/module_bindings";
import type { FileMetadata, ReplaceTicket, S3Config, UploadTicket } from "@/module_bindings/types";
import { unwrap } from "@/lib/stdb";
import { formatBytes, formatTimestamp, shortId } from "@/lib/utils";
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

const TEXT_TYPES = /^(text\/|application\/(json|xml|x-yaml|toml|javascript|typescript))/i;

async function sha256Hex(data: ArrayBuffer | Blob): Promise<string> {
  const buf = data instanceof Blob ? await data.arrayBuffer() : data;
  const digest = await crypto.subtle.digest("SHA-256", buf);
  return Array.from(new Uint8Array(digest))
    .map((b) => b.toString(16).padStart(2, "0"))
    .join("");
}

function isTextFile(meta: FileMetadata): boolean {
  const ct = meta.contentType ?? "";
  return ct.length === 0 || TEXT_TYPES.test(ct);
}

export function FilesPage() {
  const [rows, ready] = useTable(tables.my_files);
  const [cfgRows] = useTable(tables.s3_config);

  const requestUpload = useProcedure(procedures.requestUploadUrl);
  const requestDownload = useProcedure(procedures.requestDownloadUrl);
  const replaceContent = useProcedure(procedures.replaceFileContent);
  const finalizeUpload = useReducer(reducers.finalizeUpload);
  const deleteFile = useReducer(reducers.deleteFile);
  const renameFile = useReducer(reducers.renameFile);
  const updateS3 = useReducer(reducers.updateS3Config);

  const [uploadOpen, setUploadOpen] = React.useState(false);
  const [editing, setEditing] = React.useState<FileMetadata | null>(null);
  const [renaming, setRenaming] = React.useState<FileMetadata | null>(null);
  const [storageOpen, setStorageOpen] = React.useState(false);
  const [busyId, setBusyId] = React.useState<string | null>(null);

  const s3Config: S3Config | undefined = cfgRows[0] as S3Config | undefined;
  const storageReady = Boolean(s3Config && s3Config.bucket && s3Config.region);

  const files = React.useMemo(
    () => [...rows].sort((a, b) => Number(b.createdAt.microsSinceUnixEpoch - a.createdAt.microsSinceUnixEpoch)),
    [rows]
  );

  const doUpload = async (name: string, file: File): Promise<void> => {
    const contentType = file.type || undefined;
    const res = await requestUpload({ name, contentType });
    const ticket = unwrap<UploadTicket>(res);
    const putRes = await fetch(ticket.uploadUrl, {
      method: "PUT",
      body: file,
      headers: contentType ? { "Content-Type": contentType } : undefined,
    });
    if (!putRes.ok) throw new Error(`Upload failed: ${putRes.status} ${putRes.statusText}`);
    const hash = `sha256:${await sha256Hex(file)}`;
    await finalizeUpload({ fileId: ticket.fileId, hash, sizeBytes: BigInt(file.size) });
  };

  const doSaveText = async (meta: FileMetadata, text: string): Promise<void> => {
    const contentType = meta.contentType ?? "text/plain";
    const blob = new Blob([text], { type: contentType });
    const res = await replaceContent({ fileId: meta.id, contentType });
    const ticket = unwrap<ReplaceTicket>(res);
    const putRes = await fetch(ticket.uploadUrl, {
      method: "PUT",
      body: blob,
      headers: { "Content-Type": contentType },
    });
    if (!putRes.ok) throw new Error(`Save failed: ${putRes.status} ${putRes.statusText}`);
    const hash = `sha256:${await sha256Hex(blob)}`;
    await finalizeUpload({ fileId: meta.id, hash, sizeBytes: BigInt(blob.size) });
  };

  const doDownload = async (meta: FileMetadata) => {
    setBusyId(String(meta.id));
    try {
      const res = await requestDownload({ fileId: meta.id });
      const url = unwrap<string>(res);
      const r = await fetch(url);
      if (!r.ok) throw new Error(`Download failed: ${r.status}`);
      const blob = await r.blob();
      const objUrl = URL.createObjectURL(blob);
      const a = document.createElement("a");
      a.href = objUrl;
      a.download = meta.name;
      document.body.appendChild(a);
      a.click();
      a.remove();
      URL.revokeObjectURL(objUrl);
      reportSuccess(`Downloaded ${meta.name}`);
    } catch (err) {
      reportError(err);
    } finally {
      setBusyId(null);
    }
  };

  const openEditor = async (meta: FileMetadata) => {
    if (!isTextFile(meta)) {
      reportError(new Error("Only text files can be edited inline. Use download instead."));
      return;
    }
    setEditing(meta);
  };

  return (
    <div>
      <PageHeader
        title="Files"
        description="Files are stored in S3-compatible storage and tracked by SpacetimeDB. Upload, download, and edit text files inline."
        actions={
          <>
            <Button variant="outline" onClick={() => setStorageOpen(true)} className="gap-2">
              <Settings className="size-4" /> Storage
              {!storageReady ? <Badge variant="warning" className="ml-1">not configured</Badge> : null}
            </Button>
            <Button onClick={() => setUploadOpen(true)} className="gap-2" disabled={!storageReady}>
              <Upload className="size-4" /> Upload file
            </Button>
          </>
        }
      />

      {!storageReady ? (
        <div className="mb-4 rounded-lg border border-amber-500/40 bg-amber-500/10 p-4 text-sm text-amber-700 dark:text-amber-400">
          <div className="flex items-center gap-2 font-medium">
            <Cloud className="size-4" /> S3 storage is not configured
          </div>
          <p className="mt-1">
            File uploads and edits require S3 storage. Open <strong>Storage</strong> to set a bucket,
            region, and credentials.
          </p>
        </div>
      ) : null}

      <UploadDialog
        open={uploadOpen}
        onOpenChange={setUploadOpen}
        onUpload={doUpload}
      />

      <EditFileDialog
        file={editing}
        onOpenChange={(o) => !o && setEditing(null)}
        loadText={async (meta) => {
          const res = await requestDownload({ fileId: meta.id });
          const url = unwrap<string>(res);
          const r = await fetch(url);
          if (!r.ok) throw new Error(`Download failed: ${r.status}`);
          return r.text();
        }}
        onSave={doSaveText}
      />

      <RenameDialog
        file={renaming}
        onOpenChange={(o) => !o && setRenaming(null)}
        onRename={async (id, name) => {
          await renameFile({ fileId: id, name });
        }}
      />

      <StorageDialog
        open={storageOpen}
        onOpenChange={setStorageOpen}
        config={s3Config}
        onSave={async (cfg) => {
          await updateS3(cfg);
        }}
      />

      {!ready ? (
        <div className="flex justify-center p-10 text-muted-foreground">
          <Spinner className="size-5" />
        </div>
      ) : files.length === 0 ? (
        <EmptyState
          icon={FilesIcon}
          title="No files yet"
          description={storageReady ? "Upload a file to get started. Text files can be edited inline." : "Configure S3 storage first, then upload files."}
          action={
            storageReady ? (
              <Button onClick={() => setUploadOpen(true)} className="gap-2">
                <Upload className="size-4" /> Upload file
              </Button>
            ) : (
              <Button variant="outline" onClick={() => setStorageOpen(true)} className="gap-2">
                <Settings className="size-4" /> Configure storage
              </Button>
            )
          }
        />
      ) : (
        <div className="rounded-lg border">
          <Table>
            <TableHeader>
              <TableRow>
                <TableHead>Name</TableHead>
                <TableHead>Size</TableHead>
                <TableHead>Type</TableHead>
                <TableHead>Hash</TableHead>
                <TableHead>Created</TableHead>
                <TableHead className="w-40 text-right">Actions</TableHead>
              </TableRow>
            </TableHeader>
            <TableBody>
              {files.map((f) => {
                const pending = busyId === String(f.id);
                const pendingFinalize = f.hash.length === 0;
                return (
                  <TableRow key={String(f.id)}>
                    <TableCell className="font-medium">
                      {f.name}
                      {pendingFinalize ? (
                        <Badge variant="warning" className="ml-2">uploading</Badge>
                      ) : null}
                    </TableCell>
                    <TableCell className="text-muted-foreground">{formatBytes(f.sizeBytes)}</TableCell>
                    <TableCell className="text-muted-foreground">{f.contentType ?? "—"}</TableCell>
                    <TableCell className="font-mono text-[11px] text-muted-foreground">{shortId(f.hash, 10, 6)}</TableCell>
                    <TableCell className="text-muted-foreground">{formatTimestamp(f.createdAt)}</TableCell>
                    <TableCell className="text-right">
                      <div className="flex items-center justify-end gap-1">
                        <Button
                          variant="ghost"
                          size="icon"
                          aria-label="Download"
                          disabled={pendingFinalize || pending}
                          onClick={() => doDownload(f)}
                        >
                          {pending ? <Spinner /> : <Download className="size-4" />}
                        </Button>
                        <Button
                          variant="ghost"
                          size="icon"
                          aria-label="Edit text"
                          disabled={pendingFinalize || !isTextFile(f)}
                          onClick={() => openEditor(f)}
                        >
                          <Pencil className="size-4" />
                        </Button>
                        <Button
                          variant="ghost"
                          size="icon"
                          aria-label="Rename"
                          onClick={() => setRenaming(f)}
                        >
                          <Pencil className="size-3.5" />
                        </Button>
                        <ConfirmDelete
                          title={`Delete "${f.name}"?`}
                          description="This removes the file record. The underlying object stays in your S3 bucket."
                          onConfirm={async () => {
                            await deleteFile({ fileId: f.id });
                            reportSuccess("File deleted.");
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
    </div>
  );
}

function UploadDialog({
  open,
  onOpenChange,
  onUpload,
}: {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  onUpload: (name: string, file: File) => Promise<void>;
}) {
  const [file, setFile] = React.useState<File | null>(null);
  const [name, setName] = React.useState("");
  const [busy, setBusy] = React.useState(false);
  const [progress, setProgress] = React.useState("");

  React.useEffect(() => {
    if (open) {
      setFile(null);
      setName("");
      setProgress("");
    }
  }, [open]);

  const onPick = (f: File | null) => {
    setFile(f);
    setName(f?.name ?? "");
  };

  const submit = async () => {
    if (!file) return;
    setBusy(true);
    try {
      setProgress("Requesting upload URL…");
      await onUpload(name.trim() || file.name, file);
      onOpenChange(false);
      reportSuccess(`Uploaded ${file.name}`);
    } catch (err) {
      reportError(err);
    } finally {
      setBusy(false);
      setProgress("");
    }
  };

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>Upload file</DialogTitle>
          <DialogDescription>The file is streamed directly to S3 via a presigned URL.</DialogDescription>
        </DialogHeader>
        <div className="space-y-4">
          <div className="space-y-2">
            <Label htmlFor="file-pick">File</Label>
            <Input id="file-pick" type="file" onChange={(e) => onPick(e.target.files?.[0] ?? null)} disabled={busy} />
          </div>
          <div className="space-y-2">
            <Label htmlFor="file-name">Name (optional)</Label>
            <Input id="file-name" value={name} onChange={(e) => setName(e.target.value)} disabled={busy} placeholder={file?.name ?? "name"} />
          </div>
          {file ? (
            <div className="text-xs text-muted-foreground">
              {file.name} · {formatBytes(BigInt(file.size))} · {file.type || "unknown type"}
            </div>
          ) : null}
          {progress ? <div className="text-xs text-muted-foreground">{progress}</div> : null}
        </div>
        <DialogFooter>
          <Button variant="outline" onClick={() => onOpenChange(false)} disabled={busy}>
            Cancel
          </Button>
          <Button onClick={submit} disabled={busy || !file}>
            {busy ? <Spinner /> : <Upload className="size-4" />}
            Upload
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}

function EditFileDialog({
  file,
  onOpenChange,
  loadText,
  onSave,
}: {
  file: FileMetadata | null;
  onOpenChange: (open: boolean) => void;
  loadText: (meta: FileMetadata) => Promise<string>;
  onSave: (meta: FileMetadata, text: string) => Promise<void>;
}) {
  const [text, setText] = React.useState("");
  const [dirty, setDirty] = React.useState(false);
  const [loading, setLoading] = React.useState(false);
  const [busy, setBusy] = React.useState(false);

  React.useEffect(() => {
    if (!file) return;
    let cancelled = false;
    setLoading(true);
    setText("");
    setDirty(false);
    loadText(file)
      .then((t) => {
        if (!cancelled) setText(t);
      })
      .catch((err) => {
        reportError(err);
        if (!cancelled) onOpenChange(false);
      })
      .finally(() => !cancelled && setLoading(false));
    return () => {
      cancelled = true;
    };
  }, [file, loadText, onOpenChange]);

  const save = async () => {
    if (!file) return;
    setBusy(true);
    try {
      await onSave(file, text);
      setDirty(false);
      onOpenChange(false);
      reportSuccess("File saved.");
    } catch (err) {
      reportError(err);
    } finally {
      setBusy(false);
    }
  };

  return (
    <Dialog open={file !== null} onOpenChange={onOpenChange}>
      <DialogContent className="sm:max-w-3xl">
        <DialogHeader>
          <DialogTitle className="flex items-center gap-2 font-mono">
            <Pencil className="size-4" /> {file?.name ?? ""}
          </DialogTitle>
          <DialogDescription>
            {file ? `${formatBytes(file.sizeBytes)} · ${file.contentType || "text/plain"}` : ""}
          </DialogDescription>
        </DialogHeader>
        <div className="space-y-2">
          <div className="flex items-center justify-between">
            <Badge variant={dirty ? "warning" : "secondary"}>{dirty ? "unsaved" : "saved"}</Badge>
            {loading ? <span className="text-xs text-muted-foreground">Loading…</span> : null}
          </div>
          <Textarea
            value={text}
            onChange={(e) => {
              setText(e.target.value);
              setDirty(true);
            }}
            className="min-h-[420px] font-mono text-sm leading-relaxed"
            spellCheck={false}
            disabled={loading}
          />
        </div>
        <Separator />
        <DialogFooter>
          <Button
            variant="outline"
            onClick={() => onOpenChange(false)}
            disabled={busy}
          >
            <X className="size-4" /> Close
          </Button>
          <Button onClick={save} disabled={busy || loading || !dirty}>
            {busy ? <Spinner /> : <Save className="size-4" />}
            Save
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}

function RenameDialog({
  file,
  onOpenChange,
  onRename,
}: {
  file: FileMetadata | null;
  onOpenChange: (open: boolean) => void;
  onRename: (id: bigint, name: string) => Promise<void>;
}) {
  const [name, setName] = React.useState("");
  const [busy, setBusy] = React.useState(false);

  React.useEffect(() => {
    if (file) setName(file.name);
  }, [file]);

  const save = async () => {
    if (!file || !name.trim()) return;
    setBusy(true);
    try {
      await onRename(file.id, name.trim());
      onOpenChange(false);
      reportSuccess("Renamed.");
    } catch (err) {
      reportError(err);
    } finally {
      setBusy(false);
    }
  };

  return (
    <Dialog open={file !== null} onOpenChange={onOpenChange}>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>Rename file</DialogTitle>
        </DialogHeader>
        <div className="space-y-2">
          <Label htmlFor="rename-name">Name</Label>
          <Input id="rename-name" value={name} onChange={(e) => setName(e.target.value)} autoFocus />
        </div>
        <DialogFooter>
          <Button variant="outline" onClick={() => onOpenChange(false)} disabled={busy}>
            Cancel
          </Button>
          <Button onClick={save} disabled={busy || !name.trim()}>
            {busy ? <Spinner /> : null}
            Save
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}

function StorageDialog({
  open,
  onOpenChange,
  config,
  onSave,
}: {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  config: S3Config | undefined;
  onSave: (cfg: {
    bucket: string;
    region: string;
    endpoint: string | undefined;
    accessKeyId: string;
    secretAccessKey: string;
    pathPrefix: string | undefined;
    publicBaseUrl: string | undefined;
  }) => Promise<void>;
}) {
  const [bucket, setBucket] = React.useState("");
  const [region, setRegion] = React.useState("");
  const [endpoint, setEndpoint] = React.useState("");
  const [accessKeyId, setAccessKeyId] = React.useState("");
  const [secretAccessKey, setSecretAccessKey] = React.useState("");
  const [pathPrefix, setPathPrefix] = React.useState("");
  const [publicBaseUrl, setPublicBaseUrl] = React.useState("");
  const [busy, setBusy] = React.useState(false);

  React.useEffect(() => {
    if (open && config) {
      setBucket(config.bucket);
      setRegion(config.region);
      setEndpoint(config.endpoint ?? "");
      setAccessKeyId(config.accessKeyId);
      setSecretAccessKey(config.secretAccessKey);
      setPathPrefix(config.pathPrefix ?? "");
      setPublicBaseUrl(config.publicBaseUrl ?? "");
    }
  }, [open, config]);

  const save = async () => {
    setBusy(true);
    try {
      await onSave({
        bucket: bucket.trim(),
        region: region.trim(),
        endpoint: endpoint.trim() || undefined,
        accessKeyId: accessKeyId.trim(),
        secretAccessKey,
        pathPrefix: pathPrefix.trim() || undefined,
        publicBaseUrl: publicBaseUrl.trim() || undefined,
      });
      onOpenChange(false);
      reportSuccess("S3 storage configured.");
    } catch (err) {
      reportError(err);
    } finally {
      setBusy(false);
    }
  };

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="sm:max-w-lg">
        <DialogHeader>
          <DialogTitle>S3 storage settings</DialogTitle>
          <DialogDescription>
            Global S3-compatible storage used for file uploads. Credentials are stored in the module.
          </DialogDescription>
        </DialogHeader>
        <div className="grid gap-3 sm:grid-cols-2">
          <div className="space-y-2 sm:col-span-2">
            <Label htmlFor="s3-bucket">Bucket</Label>
            <Input id="s3-bucket" value={bucket} onChange={(e) => setBucket(e.target.value)} placeholder="spacenix-files" />
          </div>
          <div className="space-y-2">
            <Label htmlFor="s3-region">Region</Label>
            <Input id="s3-region" value={region} onChange={(e) => setRegion(e.target.value)} placeholder="us-east-1" />
          </div>
          <div className="space-y-2">
            <Label htmlFor="s3-endpoint">Endpoint (optional)</Label>
            <Input id="s3-endpoint" value={endpoint} onChange={(e) => setEndpoint(e.target.value)} placeholder="https://s3.example.com" />
          </div>
          <div className="space-y-2">
            <Label htmlFor="s3-key">Access key id</Label>
            <Input id="s3-key" value={accessKeyId} onChange={(e) => setAccessKeyId(e.target.value)} autoComplete="off" />
          </div>
          <div className="space-y-2">
            <Label htmlFor="s3-secret">Secret access key</Label>
            <Input id="s3-secret" type="password" value={secretAccessKey} onChange={(e) => setSecretAccessKey(e.target.value)} autoComplete="off" />
          </div>
          <div className="space-y-2">
            <Label htmlFor="s3-prefix">Path prefix (optional)</Label>
            <Input id="s3-prefix" value={pathPrefix} onChange={(e) => setPathPrefix(e.target.value)} placeholder="prod" />
          </div>
          <div className="space-y-2">
            <Label htmlFor="s3-public">Public base URL (optional)</Label>
            <Input id="s3-public" value={publicBaseUrl} onChange={(e) => setPublicBaseUrl(e.target.value)} placeholder="https://cdn.example.com" />
          </div>
        </div>
        <DialogFooter>
          <Button variant="outline" onClick={() => onOpenChange(false)} disabled={busy}>
            Cancel
          </Button>
          <Button onClick={save} disabled={busy || !bucket || !region || !accessKeyId}>
            {busy ? <Spinner /> : null}
            Save
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
