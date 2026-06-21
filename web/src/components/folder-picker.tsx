import * as React from "react";
import { Folder, FolderOpen, CornerDownRight } from "lucide-react";

import { cn } from "@/lib/utils";
import { buildTree, getParents, type TreeNode } from "@/components/file-tree-utils";
import type { FileMetadata } from "@/module_bindings/types";

export function FolderPicker({
  files,
  value,
  onChange,
  excludeId,
  className,
}: {
  files: readonly FileMetadata[];
  value: string;
  onChange: (path: string) => void;
  excludeId?: bigint;
  className?: string;
}) {
  const root = React.useMemo(() => buildTree(files), [files]);
  const directories = React.useMemo(() => collectDirs(root, excludeId), [root, excludeId]);

  return (
    <div
      className={cn(
        "max-h-64 overflow-y-auto rounded-md border bg-background p-1",
        className
      )}
    >
      <button
        type="button"
        onClick={() => onChange("")}
        className={cn(
          "flex w-full items-center gap-2 rounded-sm px-2 py-1.5 text-left text-sm transition-colors hover:bg-accent",
          value === "" && "bg-accent"
        )}
      >
        <Folder className="size-4 text-amber-500" />
        <span className="font-medium">Root</span>
      </button>
      {directories.map((dir) => (
        <FolderPickerRow
          key={dir.fullPath}
          node={dir}
          currentPath={value}
          onSelect={onChange}
        />
      ))}
      {directories.length === 0 ? (
        <p className="px-2 py-3 text-center text-xs text-muted-foreground">
          No folders yet. Create one to organize your files.
        </p>
      ) : null}
    </div>
  );
}

function collectDirs(root: TreeNode, excludeId?: bigint): TreeNode[] {
  const out: TreeNode[] = [];
  const walk = (node: TreeNode) => {
    for (const child of node.children) {
      if (child.isDirectory) {
        if (child.file && excludeId !== undefined && child.file.id === excludeId) continue;
        out.push(child);
        walk(child);
      }
    }
  };
  walk(root);
  return out;
}

function FolderPickerRow({
  node,
  currentPath,
  onSelect,
}: {
  node: TreeNode;
  currentPath: string;
  onSelect: (path: string) => void;
}) {
  const parents = getParents(node.fullPath);
  const depth = parents.length;
  const active = currentPath === node.fullPath;
  return (
    <button
      type="button"
      onClick={() => onSelect(node.fullPath)}
      className={cn(
        "flex w-full items-center gap-1.5 rounded-sm px-2 py-1.5 text-left text-sm transition-colors hover:bg-accent",
        active && "bg-accent"
      )}
      style={{ paddingLeft: `${depth * 12 + 8}px` }}
    >
      <CornerDownRight className="size-3 text-muted-foreground" />
      {active ? (
        <FolderOpen className="size-4 text-amber-500" />
      ) : (
        <Folder className="size-4 text-amber-500" />
      )}
      <span className="truncate font-mono text-xs">{node.fullPath}</span>
    </button>
  );
}
