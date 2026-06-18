import * as React from "react";
import { useProcedure, useReducer, useTable } from "spacetimedb/react";
import {
  Download,
  FolderPlus,
  FilesIcon,
  Pencil,
  Plus,
  Save,
  Upload,
  X,
  Search,
  ChevronRight,
  Home,
  Filter,
  ArrowUpDown,
  LayoutGrid,
  List as ListIcon,
} from "lucide-react";

import { procedures, reducers, tables } from "@/module_bindings";
import type { FileMetadata, ReplaceTicket, UploadTicket } from "@/module_bindings/types";
import { Timestamp } from "spacetimedb";
import { unwrap } from "@/lib/stdb";
import { cn, formatBytes, formatTimestamp } from "@/lib/utils";
import { reportError, reportSuccess } from "@/lib/toast";
import { PageHeader, EmptyState, ConfirmDelete, Spinner } from "@/components/common";
import {
  FileTreeView,
  buildTree,
  findByPath,
  joinPath,
  type TreeNode,
} from "@/components/file-tree";
import { FileRow } from "@/components/file-row";
import { FolderPicker } from "@/components/folder-picker";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Textarea } from "@/components/ui/textarea";
import { Badge } from "@/components/ui/badge";
import { Separator } from "@/components/ui/separator";
import {
  Card,
  CardContent,
} from "@/components/ui/card";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import {
  Tooltip,
  TooltipContent,
  TooltipProvider,
  TooltipTrigger,
} from "@/components/ui/tooltip";

const TEXT_TYPES = /^(text\/|application\/(json|xml|x-yaml|toml|javascript|typescript))/i;

async function sha256Hex(data: ArrayBuffer | Blob): Promise<string> {
  const buf = data instanceof Blob ? await data.arrayBuffer() : data;
  const digest = await crypto.subtle.digest("SHA-256", buf);
  return Array.from(new Uint8Array(digest))
    .map((b) => b.toString(16).padStart(2, "0"))
    .join("");
}

function isTextFile(meta: FileMetadata): boolean {
  if (meta.inlineContent != null) return true;
  const ct = meta.contentType ?? "";
  return ct.length === 0 || TEXT_TYPES.test(ct);
}

const ZERO_TS = new Timestamp(0n);

function synthesizeDir(name: string, fullPath: string): FileMetadata {
  return {
    id: 0n,
    name,
    path: fullPath,
    hash: "",
    sizeBytes: 0n,
    contentType: undefined,
    inlineContent: undefined,
    isDirectory: true,
    s3Key: "",
    createdAt: ZERO_TS,
    updatedAt: ZERO_TS,
  };
}

function isUploading(meta: FileMetadata): boolean {
  return !meta.isDirectory && meta.inlineContent == null && meta.hash.length === 0;
}

function fileFullPath(f: FileMetadata): string {
  if (f.path) return f.path;
  return f.name;
}

function fileKey(f: FileMetadata): string {
  return `${f.isDirectory ? "d" : "f"}:${String(f.id)}`;
}

type SortKey = "name" | "size" | "updated" | "kind";
type ViewMode = "list" | "grid";

export function FilesPage() {
  const [rows, ready] = useTable(tables.my_files);

  const requestUpload = useProcedure(procedures.requestUploadUrl);
  const requestDownload = useProcedure(procedures.requestDownloadUrl);
  const replaceContent = useProcedure(procedures.replaceFileContent);
  const finalizeUpload = useReducer(reducers.finalizeUpload);
  const createFolder = useReducer(reducers.createFolder);
  const deleteFile = useReducer(reducers.deleteFile);
  const renameFile = useReducer(reducers.renameFile);
  const setFileContent = useReducer(reducers.setFileContent);

  const [uploadOpen, setUploadOpen] = React.useState(false);
  const [textOpen, setTextOpen] = React.useState(false);
  const [folderOpen, setFolderOpen] = React.useState(false);
  const [editing, setEditing] = React.useState<FileMetadata | null>(null);
  const [renaming, setRenaming] = React.useState<FileMetadata | null>(null);
  const [moving, setMoving] = React.useState<FileMetadata | null>(null);
  const [busyId, setBusyId] = React.useState<string | null>(null);
  const [currentPath, setCurrentPath] = React.useState("");
  const [expanded, setExpanded] = React.useState<Set<string>>(new Set());
  const [selected, setSelected] = React.useState<string | null>(null);
  const [search, setSearch] = React.useState("");
  const [sortKey, setSortKey] = React.useState<SortKey>("name");
  const [view, setView] = React.useState<ViewMode>("list");
  const [isDragOver, setIsDragOver] = React.useState(false);
  const dragCounter = React.useRef(0);

  const tree = React.useMemo(() => buildTree(rows), [rows]);
  const currentNode = React.useMemo(
    () => findByPath(tree, currentPath) ?? tree,
    [tree, currentPath]
  );

  const directories = React.useMemo(() => {
    if (!currentNode) return [] as TreeNode[];
    return currentNode.children.filter((c) => c.isDirectory);
  }, [currentNode]);

  const filesHere = React.useMemo(() => {
    if (!currentNode) return [] as TreeNode[];
    return currentNode.children.filter((c) => !c.isDirectory);
  }, [currentNode]);

  const visibleItems = React.useMemo(() => {
    const needle = search.trim().toLowerCase();
    const filterFn = (n: TreeNode) => {
      if (!needle) return true;
      return n.name.toLowerCase().includes(needle) || n.fullPath.toLowerCase().includes(needle);
    };
    const dirs = directories.filter(filterFn);
    const files = filesHere.filter(filterFn);

    const sortFns: Record<SortKey, (a: TreeNode, b: TreeNode) => number> = {
      name: (a, b) => a.name.localeCompare(b.name),
      size: (a, b) => {
        const sa = a.file ? Number(a.file.sizeBytes) : 0;
        const sb = b.file ? Number(b.file.sizeBytes) : 0;
        return sb - sa;
      },
      updated: (a, b) => {
        const ta = a.file ? Number(a.file.updatedAt.microsSinceUnixEpoch) : 0;
        const tb = b.file ? Number(b.file.updatedAt.microsSinceUnixEpoch) : 0;
        return tb - ta;
      },
      kind: (a, b) => {
        if (a.isDirectory !== b.isDirectory) return a.isDirectory ? -1 : 1;
        return a.name.localeCompare(b.name);
      },
    };
    const sortFn = sortFns[sortKey];
    return {
      dirs: [...dirs].sort(sortFn),
      files: [...files].sort(sortFn),
    };
  }, [directories, filesHere, search, sortKey]);

  const totalSize = React.useMemo(() => {
    return rows.reduce(
      (acc, f) => (f.isDirectory ? acc : acc + Number(f.sizeBytes)),
      0
    );
  }, [rows]);

  const pendingUploads = React.useMemo(
    () => rows.filter(isUploading).length,
    [rows]
  );

  React.useEffect(() => {
    if (currentPath === "" || expanded.has(currentPath)) return;
    setExpanded((prev) => {
      const next = new Set(prev);
      next.add(currentPath);
      return next;
    });
  }, [currentPath, expanded]);

  const togglePath = React.useCallback((path: string) => {
    setExpanded((prev) => {
      const next = new Set(prev);
      if (next.has(path)) next.delete(path);
      else next.add(path);
      return next;
    });
  }, []);

  const doUpload = async (
    name: string,
    path: string | undefined,
    file: File
  ): Promise<void> => {
    const contentType = file.type || undefined;
    const res = await requestUpload({ name, path, contentType });
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
    if (meta.isDirectory) throw new Error("Folders cannot be edited as text files.");
    const contentType = meta.contentType ?? "text/plain";
    if (meta.inlineContent != null) {
      await setFileContent({
        fileId: meta.id,
        name: meta.name,
        path: meta.path ?? undefined,
        content: text,
        contentType,
      });
      return;
    }
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
    if (meta.isDirectory) return;
    setBusyId(String(meta.id));
    try {
      const blob = meta.inlineContent != null
        ? new Blob([meta.inlineContent], { type: meta.contentType ?? "text/plain" })
        : await (async () => {
            const res = await requestDownload({ fileId: meta.id });
            const url = unwrap<string>(res);
            const r = await fetch(url);
            if (!r.ok) throw new Error(`Download failed: ${r.status}`);
            return r.blob();
          })();
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
    if (meta.isDirectory) {
      reportError(new Error("Folders do not have inline content."));
      return;
    }
    if (!isTextFile(meta)) {
      reportError(new Error("Only text files can be edited."));
      return;
    }
    setEditing(meta);
  };

  const activate = React.useCallback((node: TreeNode) => {
    if (node.isDirectory) {
      setCurrentPath(node.fullPath);
      setExpanded((prev) => new Set(prev).add(node.fullPath));
      setSelected(null);
    } else if (node.file) {
      const filePath = fileFullPath(node.file);
      const segs = filePath.split("/").filter(Boolean);
      if (segs.length > 1) {
        setCurrentPath(segs.slice(0, -1).join("/"));
      }
      setSelected(fileKey(node.file));
    }
  }, []);

  const onSelect = React.useCallback((node: TreeNode) => {
    if (node.isDirectory) {
      setCurrentPath(node.fullPath);
      setSelected(null);
    } else if (node.file) {
      setSelected(fileKey(node.file));
    }
  }, []);

  const onDropFile = React.useCallback(
    async (target: TreeNode, e: React.DragEvent) => {
      e.preventDefault();
      e.stopPropagation();
      const data = e.dataTransfer.getData("application/x-spacenix-file");
      if (!data) return;
      let parsed: { id: string };
      try {
        parsed = JSON.parse(data);
      } catch {
        return;
      }
      const source = rows.find((r) => String(r.id) === parsed.id);
      if (!source) return;
      const targetPath = target.fullPath;
      const newPath =
        targetPath === ""
          ? joinPath("", source.name)
          : joinPath(targetPath, source.name);
      if (source.path === newPath) return;
      try {
        await renameFile({ fileId: source.id, name: source.name, path: newPath });
        reportSuccess(`Moved ${source.name}.`);
      } catch (err) {
        reportError(err);
      }
    },
    [renameFile, rows]
  );

  const handleGlobalDragEnter = (e: React.DragEvent) => {
    if (!Array.from(e.dataTransfer.types).includes("Files")) return;
    e.preventDefault();
    dragCounter.current += 1;
    setIsDragOver(true);
  };

  const handleGlobalDragLeave = (e: React.DragEvent) => {
    e.preventDefault();
    dragCounter.current -= 1;
    if (dragCounter.current <= 0) {
      dragCounter.current = 0;
      setIsDragOver(false);
    }
  };

  const handleGlobalDragOver = (e: React.DragEvent) => {
    if (!Array.from(e.dataTransfer.types).includes("Files")) return;
    e.preventDefault();
    e.dataTransfer.dropEffect = "copy";
  };

  const handleGlobalDrop = async (e: React.DragEvent) => {
    e.preventDefault();
    dragCounter.current = 0;
    setIsDragOver(false);
    const files = Array.from(e.dataTransfer.files);
    if (files.length === 0) return;
    for (const file of files) {
      try {
        await doUpload(file.name, currentPath || undefined, file);
        reportSuccess(`Uploaded ${file.name}`);
      } catch (err) {
        reportError(err);
      }
    }
  };

  const onDropHere = async (e: React.DragEvent) => {
    const files = Array.from(e.dataTransfer.files);
    if (files.length > 0) {
      e.preventDefault();
      e.stopPropagation();
      for (const file of files) {
        try {
          await doUpload(file.name, currentPath || undefined, file);
          reportSuccess(`Uploaded ${file.name}`);
        } catch (err) {
          reportError(err);
        }
      }
      return;
    }
    onDropFile(currentNode, e);
  };

  const breadcrumbs = React.useMemo(() => {
    if (currentPath === "") return [] as string[];
    return currentPath.split("/").filter(Boolean);
  }, [currentPath]);

  const isEmpty = rows.length === 0;

  return (
    <div
      onDragEnter={handleGlobalDragEnter}
      onDragLeave={handleGlobalDragLeave}
      onDragOver={handleGlobalDragOver}
      onDrop={handleGlobalDrop}
      className="relative"
    >
      <PageHeader
        title="Files"
        description="Sync files and folders to your devices."
        actions={
          <TooltipProvider>
            <div className="flex items-center gap-2">
              <Tooltip>
                <TooltipTrigger asChild>
                  <Button onClick={() => setUploadOpen(true)} className="gap-2">
                    <Upload className="size-4" /> Upload
                  </Button>
                </TooltipTrigger>
                <TooltipContent>Upload a file to the current folder</TooltipContent>
              </Tooltip>
              <Button variant="outline" onClick={() => setFolderOpen(true)} className="gap-2">
                <FolderPlus className="size-4" /> New folder
              </Button>
              <Button variant="outline" onClick={() => setTextOpen(true)} className="gap-2">
                <Plus className="size-4" /> New file
              </Button>
            </div>
          </TooltipProvider>
        }
      />

      {isDragOver ? (
        <div className="pointer-events-none absolute inset-0 z-40 flex items-center justify-center rounded-lg border-2 border-dashed border-primary/60 bg-primary/5 backdrop-blur-sm">
          <div className="flex flex-col items-center gap-2 text-primary">
            <Upload className="size-10" />
            <p className="text-base font-medium">Drop files to upload to {currentPath || "root"}</p>
          </div>
        </div>
      ) : null}

      <div className="grid grid-cols-1 gap-4 lg:grid-cols-[260px_1fr]">
        <Card className="hidden lg:flex lg:flex-col">
          <div className="flex items-center justify-between gap-2 border-b p-3">
            <div className="flex items-center gap-1.5 text-sm font-medium">
              <FilesIcon className="size-4" />
              Tree
            </div>
            <Badge variant="secondary" className="font-mono text-[10px]">
              {rows.length}
            </Badge>
          </div>
          <div className="max-h-[60vh] flex-1 overflow-y-auto p-2">
            {!ready ? (
              <div className="flex justify-center p-6 text-muted-foreground">
                <Spinner className="size-4" />
              </div>
            ) : (
              <FileTreeView
                root={tree}
                currentPath={currentPath}
                expanded={expanded}
                onToggle={togglePath}
                onSelect={(path, isDir) => {
                  if (isDir) {
                    setCurrentPath(path);
                    setSelected(null);
                  }
                }}
                uploadingPaths={new Set(
                  rows.filter(isUploading).map((r) => fileFullPath(r))
                )}
                pendingPaths={new Set()}
              />
            )}
          </div>
          <Separator />
          <div className="grid grid-cols-2 gap-2 p-3 text-xs">
            <Stat label="Items" value={String(rows.length)} />
            <Stat
              label="Storage"
              value={rows.length === 0 ? "0 B" : formatBytes(BigInt(totalSize))}
            />
          </div>
        </Card>

        <Card className="flex min-h-[60vh] flex-col">
          <Toolbar
            breadcrumbs={breadcrumbs}
            onGoRoot={() => {
              setCurrentPath("");
              setSelected(null);
            }}
            onGoCrumb={(i) => {
              const next = breadcrumbs.slice(0, i + 1).join("/");
              setCurrentPath(next);
              setSelected(null);
            }}
            search={search}
            onSearch={setSearch}
            sortKey={sortKey}
            onSortChange={setSortKey}
            view={view}
            onViewChange={setView}
            selected={selected}
            onClearSelection={() => setSelected(null)}
            onDeleteSelected={async () => {
              if (!selected) return;
              const f = rows.find((r) => fileKey(r) === selected);
              if (!f) return;
              await deleteFile({ fileId: f.id });
              reportSuccess("File deleted.");
              setSelected(null);
            }}
            onDownloadSelected={async () => {
              if (!selected) return;
              const f = rows.find((r) => fileKey(r) === selected);
              if (!f) return;
              await doDownload(f);
            }}
            onEditSelected={async () => {
              if (!selected) return;
              const f = rows.find((r) => fileKey(r) === selected);
              if (f) await openEditor(f);
            }}
            onRenameSelected={() => {
              if (!selected) return;
              const f = rows.find((r) => fileKey(r) === selected);
              if (f) setRenaming(f);
            }}
            onMoveSelected={() => {
              if (!selected) return;
              const f = rows.find((r) => fileKey(r) === selected);
              if (f) setMoving(f);
            }}
          />

          <CardContent
            className="flex-1 p-0"
            onDragOver={(e) => {
              if (Array.from(e.dataTransfer.types).includes("Files")) {
                e.preventDefault();
                e.dataTransfer.dropEffect = "copy";
              }
            }}
            onDrop={onDropHere}
          >
            {!ready ? (
              <div className="flex justify-center p-10 text-muted-foreground">
                <Spinner className="size-5" />
              </div>
            ) : isEmpty ? (
              <div className="p-6">
                <EmptyState
                  icon={FilesIcon}
                  title="No files yet"
                  description="Folders group files. Text files stay inline. Uploads go to storage."
                  action={
                    <div className="flex flex-wrap items-center justify-center gap-2">
                      <Button onClick={() => setFolderOpen(true)} className="gap-2">
                        <FolderPlus className="size-4" /> New folder
                      </Button>
                      <Button variant="outline" onClick={() => setTextOpen(true)} className="gap-2">
                        <Plus className="size-4" /> New text file
                      </Button>
                    </div>
                  }
                />
              </div>
            ) : visibleItems.dirs.length === 0 && visibleItems.files.length === 0 ? (
              <EmptyState
                icon={Search}
                title="No matches"
                description="Nothing in this folder matches your search."
                action={
                  <Button variant="outline" onClick={() => setSearch("")}>
                    Clear search
                  </Button>
                }
              />
            ) : (
              <div className="divide-y">
                {visibleItems.dirs.map((d) => {
                  const file = d.file ?? synthesizeDir(d.name, d.fullPath);
                  return (
                  <FileRow
                    key={String(file.id)}
                    file={file}
                    selected={selected === fileKey(d.file!)}
                    onSelect={() => onSelect(d)}
                    onActivate={() => activate(d)}
                    onDownload={() => undefined}
                    onEdit={() => undefined}
                    onRename={() => d.file && setRenaming(d.file)}
                    onMove={() => d.file && setMoving(d.file)}
                    onDelete={async () => {
                      if (!d.file) return;
                      await deleteFile({ fileId: d.file.id });
                      reportSuccess("Folder deleted.");
                    }}
                    onDragStart={(e) => {
                      if (!d.file) return;
                      e.dataTransfer.setData(
                        "application/x-spacenix-file",
                        JSON.stringify({ id: String(d.file.id) })
                      );
                      e.dataTransfer.effectAllowed = "move";
                    }}
                    onDragOver={(e) => {
                      if (!d.file) return;
                      if (Array.from(e.dataTransfer.types).includes("Files")) return;
                      e.preventDefault();
                      e.dataTransfer.dropEffect = "move";
                    }}
                    onDrop={(e) => onDropFile(d, e)}
                  />
                  );
                })}
                {visibleItems.files.map((f) => {
                  if (!f.file) return null;
                  const file = f.file;
                  return (
                    <FileRow
                      key={String(file.id)}
                      file={file}
                      selected={selected === fileKey(file)}
                      busy={busyId === String(file.id)}
                      onSelect={() => onSelect(f)}
                      onActivate={() => activate(f)}
                      onDownload={() => doDownload(file)}
                      onEdit={() => openEditor(file)}
                      onRename={() => setRenaming(file)}
                      onMove={() => setMoving(file)}
                      onDelete={async () => {
                        await deleteFile({ fileId: file.id });
                        reportSuccess("File deleted.");
                      }}
                      onDragStart={(e) => {
                        e.dataTransfer.setData(
                          "application/x-spacenix-file",
                          JSON.stringify({ id: String(file.id) })
                        );
                        e.dataTransfer.effectAllowed = "move";
                      }}
                      onDragOver={(e) => {
                        if (Array.from(e.dataTransfer.types).includes("Files")) return;
                        e.preventDefault();
                        e.dataTransfer.dropEffect = "move";
                      }}
                      onDrop={(e) => onDropFile(f, e)}
                    />
                  );
                })}
              </div>
            )}
          </CardContent>

          <div className="flex flex-wrap items-center justify-between gap-2 border-t px-3 py-2 text-xs text-muted-foreground">
            <div className="flex items-center gap-3">
              <span>
                {visibleItems.dirs.length} folder{visibleItems.dirs.length === 1 ? "" : "s"}
              </span>
              <span aria-hidden>·</span>
              <span>
                {visibleItems.files.length} file{visibleItems.files.length === 1 ? "" : "s"}
              </span>
              {pendingUploads > 0 ? (
                <>
                  <span aria-hidden>·</span>
                  <span className="inline-flex items-center gap-1 text-amber-600">
                    <Spinner className="size-3" /> {pendingUploads} uploading
                  </span>
                </>
              ) : null}
            </div>
            <div className="font-mono text-[11px]">
              {currentPath === "" ? "root" : currentPath}
            </div>
          </div>
        </Card>
      </div>

      <FolderDialog
        open={folderOpen}
        onOpenChange={setFolderOpen}
        parentPath={currentPath}
        onCreate={async (name, path) => {
          await createFolder({ name, path });
        }}
      />

      <UploadDialog
        open={uploadOpen}
        onOpenChange={setUploadOpen}
        parentPath={currentPath}
        onUpload={doUpload}
      />

      <TextFileDialog
        open={textOpen}
        onOpenChange={setTextOpen}
        parentPath={currentPath}
        onCreate={async (name, path, content) => {
          await setFileContent({
            fileId: undefined,
            name,
            path,
            content,
            contentType: "text/plain",
          });
        }}
      />

      <EditFileDialog
        file={editing}
        onOpenChange={(o) => !o && setEditing(null)}
        loadText={async (meta) => {
          if (meta.inlineContent != null) return meta.inlineContent;
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
        onRename={async (id, name, path) => {
          await renameFile({ fileId: id, name, path });
        }}
      />

      <MoveDialog
        file={moving}
        files={rows}
        onOpenChange={(o) => !o && setMoving(null)}
        onMove={async (id, name, path) => {
          await renameFile({ fileId: id, name, path });
        }}
      />
    </div>
  );
}

function Stat({ label, value }: { label: string; value: string }) {
  return (
    <div className="rounded-md border bg-muted/30 p-2">
      <div className="text-[10px] uppercase tracking-wide text-muted-foreground">{label}</div>
      <div className="mt-0.5 truncate font-mono text-sm">{value}</div>
    </div>
  );
}

function Toolbar({
  breadcrumbs,
  onGoRoot,
  onGoCrumb,
  search,
  onSearch,
  sortKey,
  onSortChange,
  view,
  onViewChange,
  selected,
  onClearSelection,
  onDeleteSelected,
  onDownloadSelected,
  onEditSelected,
  onRenameSelected,
  onMoveSelected,
}: {
  breadcrumbs: string[];
  onGoRoot: () => void;
  onGoCrumb: (i: number) => void;
  search: string;
  onSearch: (s: string) => void;
  sortKey: SortKey;
  onSortChange: (s: SortKey) => void;
  view: ViewMode;
  onViewChange: (v: ViewMode) => void;
  selected: string | null;
  onClearSelection: () => void;
  onDeleteSelected: () => void | Promise<void>;
  onDownloadSelected: () => void | Promise<void>;
  onEditSelected: () => void | Promise<void>;
  onRenameSelected: () => void;
  onMoveSelected: () => void;
}) {
  return (
    <div className="flex flex-col gap-2 border-b p-3">
      <div className="flex flex-wrap items-center gap-2">
        <div className="flex min-w-0 flex-1 items-center gap-1 overflow-x-auto rounded-md border bg-muted/30 px-2 py-1.5 text-sm">
          <button
            type="button"
            onClick={onGoRoot}
            className="inline-flex items-center gap-1 rounded px-1.5 py-0.5 hover:bg-accent"
          >
            <Home className="size-3.5" /> root
          </button>
          {breadcrumbs.map((seg, i) => (
            <React.Fragment key={`${seg}-${i}`}>
              <ChevronRight className="size-3.5 shrink-0 text-muted-foreground" />
              <button
                type="button"
                onClick={() => onGoCrumb(i)}
                className={cn(
                  "shrink-0 rounded px-1.5 py-0.5 hover:bg-accent",
                  i === breadcrumbs.length - 1 && "font-medium"
                )}
              >
                {seg}
              </button>
            </React.Fragment>
          ))}
        </div>

        <div className="relative w-full sm:w-56">
          <Search className="pointer-events-none absolute left-2 top-1/2 size-3.5 -translate-y-1/2 text-muted-foreground" />
          <Input
            value={search}
            onChange={(e) => onSearch(e.target.value)}
            placeholder="Search this folder…"
            className="h-8 pl-7 text-xs"
          />
        </div>

        <SortMenu sortKey={sortKey} onChange={onSortChange} />

        <div className="flex items-center rounded-md border bg-muted/30 p-0.5">
          <button
            type="button"
            aria-label="List view"
            onClick={() => onViewChange("list")}
            className={cn(
              "inline-flex size-7 items-center justify-center rounded text-muted-foreground transition-colors",
              view === "list" && "bg-background text-foreground shadow-xs"
            )}
          >
            <ListIcon className="size-3.5" />
          </button>
          <button
            type="button"
            aria-label="Grid view"
            onClick={() => onViewChange("grid")}
            className={cn(
              "inline-flex size-7 items-center justify-center rounded text-muted-foreground transition-colors",
              view === "grid" && "bg-background text-foreground shadow-xs"
            )}
          >
            <LayoutGrid className="size-3.5" />
          </button>
        </div>
      </div>

      {selected ? (
        <div className="flex flex-wrap items-center gap-1.5 rounded-md border border-primary/30 bg-primary/5 p-2 text-xs">
          <span className="font-medium">1 selected</span>
          <span aria-hidden>·</span>
          <button
            type="button"
            className="rounded px-1.5 py-0.5 hover:bg-accent"
            onClick={() => void onDownloadSelected()}
          >
            <Download className="mr-1 inline size-3" /> Download
          </button>
          <button
            type="button"
            className="rounded px-1.5 py-0.5 hover:bg-accent"
            onClick={() => void onEditSelected()}
          >
            <Pencil className="mr-1 inline size-3" /> Edit
          </button>
          <button
            type="button"
            className="rounded px-1.5 py-0.5 hover:bg-accent"
            onClick={onRenameSelected}
          >
            <Pencil className="mr-1 inline size-3" /> Rename
          </button>
          <button
            type="button"
            className="rounded px-1.5 py-0.5 hover:bg-accent"
            onClick={onMoveSelected}
          >
            Move…
          </button>
          <ConfirmDelete
            title="Delete selected file?"
            description="This removes the file from syncing."
            trigger={
              <button type="button" className="rounded px-1.5 py-0.5 text-destructive hover:bg-destructive/10">
                Delete
              </button>
            }
            onConfirm={onDeleteSelected}
          />
          <div className="flex-1" />
          <button
            type="button"
            onClick={onClearSelection}
            className="rounded px-1.5 py-0.5 text-muted-foreground hover:bg-accent"
          >
            <X className="inline size-3" /> clear
          </button>
        </div>
      ) : null}

      {view === "grid" ? (
        <p className="text-[11px] text-muted-foreground">
          Grid view falls back to the list layout for now. The browser view is the canonical one.
        </p>
      ) : null}
    </div>
  );
}

function SortMenu({
  sortKey,
  onChange,
}: {
  sortKey: SortKey;
  onChange: (s: SortKey) => void;
}) {
  const options: { key: SortKey; label: string }[] = [
    { key: "name", label: "Name" },
    { key: "updated", label: "Recently updated" },
    { key: "size", label: "Size" },
    { key: "kind", label: "Kind" },
  ];
  const current = options.find((o) => o.key === sortKey) ?? options[0];
  const [open, setOpen] = React.useState(false);
  const ref = React.useRef<HTMLDivElement | null>(null);

  React.useEffect(() => {
    if (!open) return;
    const onDoc = (e: MouseEvent) => {
      if (!ref.current?.contains(e.target as Node)) setOpen(false);
    };
    document.addEventListener("mousedown", onDoc);
    return () => document.removeEventListener("mousedown", onDoc);
  }, [open]);

  return (
    <div ref={ref} className="relative">
      <button
        type="button"
        onClick={() => setOpen((v) => !v)}
        className="inline-flex h-8 items-center gap-1.5 rounded-md border bg-background px-2.5 text-xs hover:bg-accent"
        aria-expanded={open}
      >
        <ArrowUpDown className="size-3.5 text-muted-foreground" />
        <span className="text-muted-foreground">Sort:</span>
        <span className="font-medium">{current.label}</span>
      </button>
      {open ? (
        <div className="absolute right-0 z-20 mt-1 w-44 overflow-hidden rounded-md border bg-popover p-1 text-popover-foreground shadow-md">
          {options.map((o) => (
            <button
              key={o.key}
              type="button"
              onClick={() => {
                onChange(o.key);
                setOpen(false);
              }}
              className={cn(
                "flex w-full items-center gap-2 rounded-sm px-2 py-1.5 text-left text-sm hover:bg-accent",
                sortKey === o.key && "bg-accent"
              )}
            >
              {o.label}
            </button>
          ))}
        </div>
      ) : null}
    </div>
  );
}

function UploadDialog({
  open,
  onOpenChange,
  onUpload,
  parentPath,
}: {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  onUpload: (name: string, path: string | undefined, file: File) => Promise<void>;
  parentPath: string;
}) {
  const [file, setFile] = React.useState<File | null>(null);
  const [name, setName] = React.useState("");
  const [path, setPath] = React.useState("");
  const [busy, setBusy] = React.useState(false);
  const [progress, setProgress] = React.useState("");

  React.useEffect(() => {
    if (open) {
      setFile(null);
      setName("");
      setPath(parentPath);
      setProgress("");
    }
  }, [open, parentPath]);

  const onPick = (f: File | null) => {
    setFile(f);
    setName(f?.name ?? "");
  };

  const submit = async () => {
    if (!file) return;
    setBusy(true);
    try {
      setProgress("Requesting upload URL…");
      const finalPath = path.trim() || parentPath || undefined;
      await onUpload(name.trim() || file.name, finalPath, file);
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
          <DialogDescription>
            {parentPath
              ? `Uploads go to ${parentPath}/.`
              : "Uploads go to storage at the root."}
          </DialogDescription>
        </DialogHeader>
        <div className="space-y-4">
          <div className="space-y-2">
            <Label htmlFor="file-pick">File</Label>
            <Input
              id="file-pick"
              type="file"
              onChange={(e) => onPick(e.target.files?.[0] ?? null)}
              disabled={busy}
            />
          </div>
          <div className="space-y-2">
            <Label htmlFor="file-name">Name</Label>
            <Input
              id="file-name"
              value={name}
              onChange={(e) => setName(e.target.value)}
              disabled={busy}
              placeholder={file?.name ?? "name"}
            />
          </div>
          <div className="space-y-2">
            <Label htmlFor="file-path">Path</Label>
            <Input
              id="file-path"
              value={path}
              onChange={(e) => setPath(e.target.value)}
              disabled={busy}
              placeholder={parentPath || "/etc/nixos/foo.nix"}
              className="font-mono"
            />
            <p className="text-[11px] text-muted-foreground">
              Leave blank to upload to {parentPath || "root"}.
            </p>
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

function FolderDialog({
  open,
  onOpenChange,
  onCreate,
  parentPath,
}: {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  onCreate: (name: string, path: string | undefined) => Promise<void>;
  parentPath: string;
}) {
  const [name, setName] = React.useState("");
  const [path, setPath] = React.useState("");
  const [busy, setBusy] = React.useState(false);

  React.useEffect(() => {
    if (open) {
      setName("");
      setPath(parentPath);
    }
  }, [open, parentPath]);

  const submit = async () => {
    const trimmedPath = path.trim();
    const trimmedName =
      name.trim() || trimmedPath.split("/").filter(Boolean).pop() || "folder";
    setBusy(true);
    try {
      await onCreate(trimmedName, trimmedPath || undefined);
      onOpenChange(false);
      reportSuccess("Folder created.");
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
          <DialogTitle>New folder</DialogTitle>
          <DialogDescription>
            {parentPath
              ? `Creates a folder inside ${parentPath}/.`
              : "Creates a folder at the root."}
          </DialogDescription>
        </DialogHeader>
        <div className="space-y-4">
          <div className="space-y-2">
            <Label htmlFor="folder-name">Name</Label>
            <Input
              id="folder-name"
              value={name}
              onChange={(e) => setName(e.target.value)}
              placeholder="config"
              autoFocus
            />
          </div>
          <div className="space-y-2">
            <Label htmlFor="folder-path">Path</Label>
            <Input
              id="folder-path"
              value={path}
              onChange={(e) => setPath(e.target.value)}
              placeholder={parentPath || "/etc/nixos"}
              className="font-mono"
            />
            <p className="text-[11px] text-muted-foreground">
              Path is the full path the folder will sync to.
            </p>
          </div>
        </div>
        <DialogFooter>
          <Button variant="outline" onClick={() => onOpenChange(false)} disabled={busy}>
            Cancel
          </Button>
          <Button onClick={submit} disabled={busy || (!name.trim() && !path.trim())}>
            {busy ? <Spinner /> : <FolderPlus className="size-4" />}
            Create
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}

function TextFileDialog({
  open,
  onOpenChange,
  onCreate,
  parentPath,
}: {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  onCreate: (name: string, path: string | undefined, content: string) => Promise<void>;
  parentPath: string;
}) {
  const [name, setName] = React.useState("");
  const [path, setPath] = React.useState("");
  const [content, setContent] = React.useState("");
  const [busy, setBusy] = React.useState(false);

  React.useEffect(() => {
    if (open) {
      setName("");
      setPath(parentPath);
      setContent("");
    }
  }, [open, parentPath]);

  const submit = async () => {
    const trimmedName =
      name.trim() || path.trim().split("/").filter(Boolean).pop() || "text-file";
    setBusy(true);
    try {
      await onCreate(trimmedName, path.trim() || undefined, content);
      onOpenChange(false);
      reportSuccess("Text file created.");
    } catch (err) {
      reportError(err);
    } finally {
      setBusy(false);
    }
  };

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="sm:max-w-3xl">
        <DialogHeader>
          <DialogTitle>New text file</DialogTitle>
          <DialogDescription>
            Inline content stays on the server. Files over 256 KiB should be uploaded instead.
          </DialogDescription>
        </DialogHeader>
        <div className="space-y-4">
          <div className="grid gap-3 sm:grid-cols-2">
            <div className="space-y-2">
              <Label htmlFor="text-name">Name</Label>
              <Input
                id="text-name"
                value={name}
                onChange={(e) => setName(e.target.value)}
                placeholder="config.toml"
              />
            </div>
            <div className="space-y-2">
              <Label htmlFor="text-path">Path</Label>
              <Input
                id="text-path"
                value={path}
                onChange={(e) => setPath(e.target.value)}
                placeholder={parentPath || "/etc/nixos/configuration.nix"}
                className="font-mono"
              />
            </div>
          </div>
          <div className="space-y-2">
            <Label htmlFor="text-content">Content</Label>
            <Textarea
              id="text-content"
              value={content}
              onChange={(e) => setContent(e.target.value)}
              className="min-h-[360px] font-mono text-sm leading-relaxed"
              spellCheck={false}
            />
          </div>
        </div>
        <DialogFooter>
          <Button variant="outline" onClick={() => onOpenChange(false)} disabled={busy}>
            Cancel
          </Button>
          <Button onClick={submit} disabled={busy || (!name.trim() && !path.trim())}>
            {busy ? <Spinner /> : <Plus className="size-4" />}
            Create
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
            <Badge variant={dirty ? "warning" : "secondary"}>
              {dirty ? "unsaved" : "saved"}
            </Badge>
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
          <Button variant="outline" onClick={() => onOpenChange(false)} disabled={busy}>
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
  onRename: (id: bigint, name: string, path: string | undefined) => Promise<void>;
}) {
  const [name, setName] = React.useState("");
  const [path, setPath] = React.useState("");
  const [busy, setBusy] = React.useState(false);

  React.useEffect(() => {
    if (file) {
      setName(file.name);
      setPath(file.path ?? "");
    }
  }, [file]);

  const save = async () => {
    if (!file || !name.trim()) return;
    setBusy(true);
    try {
      await onRename(file.id, name.trim(), path.trim() || undefined);
      onOpenChange(false);
      reportSuccess("File metadata saved.");
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
          <DialogTitle>Edit file metadata</DialogTitle>
          <DialogDescription>
            Update the name and where the file syncs to.
          </DialogDescription>
        </DialogHeader>
        <div className="space-y-4">
          <div className="space-y-2">
            <Label htmlFor="rename-name">Name</Label>
            <Input
              id="rename-name"
              value={name}
              onChange={(e) => setName(e.target.value)}
              autoFocus
            />
          </div>
          <div className="space-y-2">
            <Label htmlFor="rename-path">Path</Label>
            <Input
              id="rename-path"
              value={path}
              onChange={(e) => setPath(e.target.value)}
              placeholder="/etc/nixos/configuration.nix"
              className="font-mono"
            />
          </div>
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

function MoveDialog({
  file,
  files,
  onOpenChange,
  onMove,
}: {
  file: FileMetadata | null;
  files: readonly FileMetadata[];
  onOpenChange: (open: boolean) => void;
  onMove: (id: bigint, name: string, path: string | undefined) => Promise<void>;
}) {
  const [target, setTarget] = React.useState("");
  const [busy, setBusy] = React.useState(false);

  React.useEffect(() => {
    if (file) {
      const segs = (file.path ?? "").split("/").filter(Boolean);
      setTarget(segs.length > 1 ? segs.slice(0, -1).join("/") : "");
    }
  }, [file]);

  const submit = async () => {
    if (!file) return;
    const newPath = target === "" ? file.name : joinPath(target, file.name);
    setBusy(true);
    try {
      await onMove(file.id, file.name, newPath);
      onOpenChange(false);
      reportSuccess(`Moved ${file.name}.`);
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
          <DialogTitle>Move {file?.name ?? ""}</DialogTitle>
          <DialogDescription>
            Pick a destination folder. The file keeps its name.
          </DialogDescription>
        </DialogHeader>
        <div className="space-y-3">
          <div className="rounded-md border bg-muted/30 p-2 text-xs">
            <div className="text-muted-foreground">Current path</div>
            <div className="mt-0.5 truncate font-mono">{file?.path || "—"}</div>
          </div>
          <div className="space-y-2">
            <Label>Destination folder</Label>
            <FolderPicker
              files={files}
              value={target}
              onChange={setTarget}
              excludeId={file?.id}
            />
            <p className="text-[11px] text-muted-foreground">
              New path will be{" "}
              <code className="font-mono">
                {target === "" ? file?.name : `${target}/${file?.name}`}
              </code>
            </p>
          </div>
        </div>
        <DialogFooter>
          <Button variant="outline" onClick={() => onOpenChange(false)} disabled={busy}>
            Cancel
          </Button>
          <Button onClick={submit} disabled={busy}>
            {busy ? <Spinner /> : null}
            Move
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
