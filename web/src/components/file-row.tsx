import * as React from "react";
import {
  Download,
  Edit3,
  FileText,
  Folder,
  File as FileIcon,
  Loader2,
  MoreHorizontal,
  Pencil,
  Trash2,
  FolderInput,
  ChevronRight,
} from "lucide-react";

import { cn, formatBytes, formatTimestamp, shortId } from "@/lib/utils";
import { Button } from "@/components/ui/button";
import { Spinner } from "@/components/common";
import { Badge } from "@/components/ui/badge";
import {
  AlertDialog,
  AlertDialogAction,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
} from "@/components/ui/alert-dialog";
import type { FileMetadata } from "@/module_bindings/types";

function fileKind(meta: FileMetadata): "folder" | "text" | "file" | "uploading" {
  if (meta.isDirectory) return "folder";
  const pendingFinalize = !meta.isDirectory && meta.inlineContent == null && meta.hash.length === 0;
  if (pendingFinalize) return "uploading";
  if (meta.inlineContent != null) return "text";
  return "file";
}

export function FileRow({
  file,
  selected,
  onSelect,
  onActivate,
  onDownload,
  onEdit,
  onRename,
  onMove,
  onDelete,
  busy,
  dragging,
  onDragStart,
  onDragEnd,
  onDragOver,
  onDrop,
}: {
  file: FileMetadata;
  selected: boolean;
  onSelect: () => void;
  onActivate: () => void;
  onDownload: () => void;
  onEdit: () => void;
  onRename: () => void;
  onMove: () => void;
  onDelete: () => void;
  busy?: boolean;
  dragging?: boolean;
  onDragStart?: (e: React.DragEvent) => void;
  onDragEnd?: (e: React.DragEvent) => void;
  onDragOver?: (e: React.DragEvent) => void;
  onDrop?: (e: React.DragEvent) => void;
}) {
  const kind = fileKind(file);
  const isFolder = kind === "folder";
  const isUploading = kind === "uploading";
  const [menuOpen, setMenuOpen] = React.useState(false);
  const menuRef = React.useRef<HTMLDivElement | null>(null);
  const triggerRef = React.useRef<HTMLButtonElement | null>(null);

  React.useEffect(() => {
    if (!menuOpen) return;
    const onDocClick = (e: MouseEvent) => {
      const t = e.target as Node;
      if (menuRef.current?.contains(t)) return;
      if (triggerRef.current?.contains(t)) return;
      setMenuOpen(false);
    };
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") setMenuOpen(false);
    };
    document.addEventListener("mousedown", onDocClick);
    document.addEventListener("keydown", onKey);
    return () => {
      document.removeEventListener("mousedown", onDocClick);
      document.removeEventListener("keydown", onKey);
    };
  }, [menuOpen]);

  const handleKey = (e: React.KeyboardEvent) => {
    if (e.key === "Enter") {
      e.preventDefault();
      onActivate();
    } else if (e.key === " ") {
      e.preventDefault();
      onSelect();
    }
  };

  return (
    <div
      role="row"
      aria-selected={selected}
      tabIndex={0}
      onKeyDown={handleKey}
      onClick={onSelect}
      onDoubleClick={onActivate}
      draggable={Boolean(onDragStart)}
      onDragStart={onDragStart}
      onDragEnd={onDragEnd}
      onDragOver={onDragOver}
      onDrop={onDrop}
      className={cn(
        "group grid cursor-pointer grid-cols-[1fr_auto_120px_120px_44px] items-center gap-3 rounded-md border border-transparent px-3 py-2 text-sm transition-colors",
        "hover:bg-accent/40 focus:bg-accent/40 focus:outline-none",
        selected && "border-primary/40 bg-primary/5",
        dragging && "opacity-50"
      )}
    >
      <div className="flex min-w-0 items-center gap-2.5">
        <span
          className={cn(
            "flex size-7 shrink-0 items-center justify-center rounded-md",
            isFolder
              ? "bg-amber-500/10 text-amber-600 dark:text-amber-400"
              : kind === "text"
                ? "bg-sky-500/10 text-sky-600 dark:text-sky-400"
                : "bg-muted text-muted-foreground"
          )}
        >
          {isFolder ? (
            <Folder className="size-4" />
          ) : kind === "text" ? (
            <FileText className="size-4" />
          ) : (
            <FileIcon className="size-4" />
          )}
        </span>
        <div className="min-w-0">
          <div className="flex items-center gap-2">
            <span className="truncate font-medium">{file.name}</span>
            {isUploading ? (
              <Badge variant="warning" className="gap-1">
                <Loader2 className="size-3 animate-spin" /> uploading
              </Badge>
            ) : null}
          </div>
          {file.contentType ? (
            <div className="truncate text-[11px] text-muted-foreground">{file.contentType}</div>
          ) : file.path ? (
            <div className="truncate font-mono text-[11px] text-muted-foreground">{file.path}</div>
          ) : null}
        </div>
      </div>

      <div className="hidden text-right text-xs text-muted-foreground md:block">
        {file.hash ? (
          <code className="font-mono text-[11px]">{shortId(file.hash, 10, 6)}</code>
        ) : (
          <span>—</span>
        )}
      </div>

      <div className="hidden text-right text-xs text-muted-foreground md:block">
        {isFolder ? "—" : formatBytes(file.sizeBytes)}
      </div>

      <div className="hidden text-right text-xs text-muted-foreground md:block">
        {formatTimestamp(file.updatedAt)}
      </div>

      <div className="flex items-center justify-end gap-1" onClick={(e) => e.stopPropagation()}>
        {busy ? (
          <Spinner className="size-4" />
        ) : isFolder ? (
          <Button
            variant="ghost"
            size="icon"
            className="size-7"
            aria-label="Open folder"
            onClick={onActivate}
          >
            <ChevronRight className="size-4" />
          </Button>
        ) : (
          <Button
            variant="ghost"
            size="icon"
            className="size-7"
            aria-label="Download"
            disabled={isUploading}
            onClick={onDownload}
          >
            <Download className="size-4" />
          </Button>
        )}
        <div className="relative">
          <Button
            ref={triggerRef}
            variant="ghost"
            size="icon"
            className="size-7"
            aria-label="More actions"
            onClick={() => setMenuOpen((v) => !v)}
            aria-expanded={menuOpen}
          >
            <MoreHorizontal className="size-4" />
          </Button>
          {menuOpen ? (
            <div
              ref={menuRef}
              role="menu"
              className="absolute right-0 z-30 mt-1 w-44 overflow-hidden rounded-md border bg-popover p-1 text-popover-foreground shadow-md animate-in fade-in-0 zoom-in-95"
            >
              {isFolder ? (
                <MenuItem
                  icon={<Folder className="size-3.5" />}
                  label="Open"
                  onClick={() => {
                    setMenuOpen(false);
                    onActivate();
                  }}
                />
              ) : (
                <MenuItem
                  icon={<Download className="size-3.5" />}
                  label="Download"
                  disabled={isUploading}
                  onClick={() => {
                    setMenuOpen(false);
                    onDownload();
                  }}
                />
              )}
              {!isFolder ? (
                <MenuItem
                  icon={<Edit3 className="size-3.5" />}
                  label="Edit text"
                  onClick={() => {
                    setMenuOpen(false);
                    onEdit();
                  }}
                />
              ) : null}
              <MenuItem
                icon={<Pencil className="size-3.5" />}
                label="Rename"
                onClick={() => {
                  setMenuOpen(false);
                  onRename();
                }}
              />
              <MenuItem
                icon={<FolderInput className="size-3.5" />}
                label="Move…"
                onClick={() => {
                  setMenuOpen(false);
                  onMove();
                }}
              />
              <div className="my-1 h-px bg-border" />
              <DeleteMenuItem
                onConfirm={() => {
                  setMenuOpen(false);
                  onDelete();
                }}
                name={file.name}
                isFolder={isFolder}
              />
            </div>
          ) : null}
        </div>
      </div>
    </div>
  );
}

function MenuItem({
  icon,
  label,
  onClick,
  disabled,
  destructive,
}: {
  icon: React.ReactNode;
  label: string;
  onClick: () => void;
  disabled?: boolean;
  destructive?: boolean;
}) {
  return (
    <button
      type="button"
      role="menuitem"
      onClick={onClick}
      disabled={disabled}
      className={cn(
        "flex w-full items-center gap-2 rounded-sm px-2 py-1.5 text-left text-sm transition-colors",
        "hover:bg-accent hover:text-accent-foreground focus:bg-accent focus:text-accent-foreground",
        "disabled:pointer-events-none disabled:opacity-50",
        destructive && "text-destructive hover:text-destructive"
      )}
    >
      <span className="text-muted-foreground">{icon}</span>
      {label}
    </button>
  );
}

function DeleteMenuItem({
  onConfirm,
  name,
  isFolder,
}: {
  onConfirm: () => void;
  name: string;
  isFolder: boolean;
}) {
  const [open, setOpen] = React.useState(false);
  return (
    <>
      <button
        type="button"
        role="menuitem"
        onClick={() => setOpen(true)}
        className="flex w-full items-center gap-2 rounded-sm px-2 py-1.5 text-left text-sm text-destructive transition-colors hover:bg-destructive/10 focus:bg-destructive/10"
      >
        <span>
          <Trash2 className="size-3.5" />
        </span>
        Delete
      </button>
      <AlertDialog open={open} onOpenChange={setOpen}>
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>Delete "{name}"?</AlertDialogTitle>
            <AlertDialogDescription>
              {isFolder
                ? "This removes the folder from syncing. Files inside stay in their place."
                : "This removes the file from syncing. The stored object stays in place."}
            </AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel>Cancel</AlertDialogCancel>
            <AlertDialogAction
              onClick={() => {
                onConfirm();
                setOpen(false);
              }}
              className="bg-destructive text-destructive-foreground hover:bg-destructive/90"
            >
              Delete
            </AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
    </>
  );
}
