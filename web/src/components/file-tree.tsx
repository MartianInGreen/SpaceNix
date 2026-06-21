import * as React from "react";
import {
  ChevronDown,
  ChevronRight,
  Folder,
  FolderOpen,
  FileText,
  File as FileIcon,
  Loader2,
} from "lucide-react";

import { cn } from "@/lib/utils";
import type { FileMetadata } from "@/module_bindings/types";
import { Spinner } from "@/components/common";

export type TreeNode = {
  name: string;
  fullPath: string;
  isDirectory: boolean;
  /** True when the directory was implied by file paths but has no `user_file` row. */
  implicit?: boolean;
  file?: FileMetadata;
  children: TreeNode[];
};

export function buildTree(files: readonly FileMetadata[]): TreeNode {
  const root: TreeNode = { name: "", fullPath: "", isDirectory: true, children: [] };
  const dirMap = new Map<string, TreeNode>([["", root]]);

  const sorted = [...files].sort((a, b) => {
    if (a.isDirectory !== b.isDirectory) return a.isDirectory ? -1 : 1;
    return a.name.localeCompare(b.name);
  });

  for (const f of sorted) {
    const rawPath = f.treePath ?? "";
    const segments = rawPath === "" ? [] : rawPath.split("/").filter(Boolean);
    let parentPath = "";
    let parent = root;
    for (const seg of segments) {
      parentPath = parentPath === "" ? seg : `${parentPath}/${seg}`;
      let next = dirMap.get(parentPath);
      if (!next) {
        next = {
          name: seg,
          fullPath: parentPath,
          isDirectory: true,
          implicit: true,
          children: [],
        };
        parent.children.push(next);
        dirMap.set(parentPath, next);
      }
      parent = next;
    }
    const name = segments.length > 0 ? segments[segments.length - 1] : f.name;
    const node: TreeNode = {
      name,
      fullPath: rawPath === "" ? f.name : rawPath,
      isDirectory: f.isDirectory,
      children: [],
    };
    if (f.isDirectory) {
      node.file = f;
      // Replace the implicit placeholder (if any) at this path with the explicit one.
      const existingIdx = parent.children.findIndex(
        (c) => c.isDirectory && c.fullPath === rawPath
      );
      if (existingIdx >= 0) {
        node.children = parent.children[existingIdx].children;
        parent.children[existingIdx] = node;
        dirMap.set(rawPath, node);
      } else {
        parent.children.push(node);
        dirMap.set(rawPath, node);
      }
    } else {
      node.file = f;
      parent.children.push(node);
    }
  }

  return root;
}

export function flattenTree(node: TreeNode, depth = 0): Array<TreeNode & { depth: number }> {
  const out: Array<TreeNode & { depth: number }> = [];
  for (const child of node.children) {
    out.push({ ...child, depth });
    if (child.isDirectory && child.children.length > 0) {
      out.push(...flattenTree(child, depth + 1));
    }
  }
  return out;
}

export function findByPath(root: TreeNode, fullPath: string): TreeNode | null {
  if (fullPath === "") return root;
  const segments = fullPath.split("/").filter(Boolean);
  let current: TreeNode | undefined = root;
  for (const seg of segments) {
    if (!current) return null;
    current = current.children.find((c) => c.isDirectory && c.name === seg);
  }
  return current ?? null;
}

export function getParents(fullPath: string): string[] {
  if (fullPath === "") return [];
  const segments = fullPath.split("/").filter(Boolean);
  const out: string[] = [];
  for (let i = 1; i <= segments.length; i++) {
    out.push(segments.slice(0, i).join("/"));
  }
  return out;
}

export function joinPath(parent: string, name: string): string {
  if (parent === "") return name;
  return `${parent}/${name}`;
}

export function basename(fullPath: string, fallback: string): string {
  if (!fullPath) return fallback;
  const segs = fullPath.split("/").filter(Boolean);
  return segs[segs.length - 1] ?? fallback;
}

type TreeProps = {
  root: TreeNode;
  currentPath: string;
  expanded: Set<string>;
  onToggle: (path: string) => void;
  onSelect: (path: string, isDirectory: boolean) => void;
  uploadingPaths: Set<string>;
  pendingPaths: Set<string>;
  className?: string;
};

export function FileTreeView({
  root,
  currentPath,
  expanded,
  onToggle,
  onSelect,
  uploadingPaths,
  pendingPaths,
  className,
}: TreeProps) {
  const nodes = React.useMemo(() => {
    const out: Array<{ node: TreeNode; depth: number; isLast: boolean }> = [];
    const walk = (parent: TreeNode, depth: number) => {
      const children = parent.children;
      children.forEach((child, idx) => {
        out.push({ node: child, depth, isLast: idx === children.length - 1 });
        if (child.isDirectory && expanded.has(child.fullPath)) {
          walk(child, depth + 1);
        }
      });
    };
    walk(root, 0);
    return out;
  }, [root, expanded]);

  return (
    <div className={cn("text-sm", className)}>
      <TreeRow
        node={root}
        depth={0}
        isLast={false}
        currentPath={currentPath}
        expanded={expanded}
        onToggle={onToggle}
        onSelect={onSelect}
        uploadingPaths={uploadingPaths}
        pendingPaths={pendingPaths}
        isRoot
      />
      {nodes.map(({ node, depth, isLast }) => (
        <TreeRow
          key={node.fullPath || node.name || "__root__"}
          node={node}
          depth={depth + 1}
          isLast={isLast}
          currentPath={currentPath}
          expanded={expanded}
          onToggle={onToggle}
          onSelect={onSelect}
          uploadingPaths={uploadingPaths}
          pendingPaths={pendingPaths}
        />
      ))}
    </div>
  );
}

function TreeRow({
  node,
  depth,
  currentPath,
  expanded,
  onToggle,
  onSelect,
  uploadingPaths,
  pendingPaths,
  isRoot = false,
}: {
  node: TreeNode;
  depth: number;
  isLast: boolean;
  currentPath: string;
  expanded: Set<string>;
  onToggle: (path: string) => void;
  onSelect: (path: string, isDirectory: boolean) => void;
  uploadingPaths: Set<string>;
  pendingPaths: Set<string>;
  isRoot?: boolean;
}) {
  const isExpanded = expanded.has(node.fullPath);
  const isActive = currentPath === node.fullPath;
  const isDir = node.isDirectory;
  const pending = pendingPaths.has(node.fullPath);
  const uploading = uploadingPaths.has(node.fullPath);

  return (
    <button
      type="button"
      onClick={() => {
        if (isDir && !isRoot) onToggle(node.fullPath);
        onSelect(node.fullPath, isDir);
      }}
      className={cn(
        "group flex w-full items-center gap-1 rounded-md px-1.5 py-1 text-left transition-colors",
        isActive
          ? "bg-sidebar-accent text-sidebar-accent-foreground"
          : "text-sidebar-foreground/80 hover:bg-sidebar-accent/60 hover:text-sidebar-accent-foreground"
      )}
      style={{ paddingLeft: `${depth * 14 + 6}px` }}
    >
      <span className="flex size-3.5 shrink-0 items-center justify-center text-muted-foreground">
        {isDir && !isRoot ? (
          isExpanded ? (
            <ChevronDown className="size-3.5" />
          ) : (
            <ChevronRight className="size-3.5" />
          )
        ) : null}
      </span>
      <span className="flex size-4 shrink-0 items-center justify-center text-muted-foreground">
        {isDir ? (
          isExpanded ? (
            <FolderOpen className={cn("size-4", node.implicit && "opacity-60")} />
          ) : (
            <Folder className={cn("size-4", node.implicit && "opacity-60")} />
          )
        ) : isRoot ? null : (() => {
          const ct = node.file?.contentType ?? "";
          if (!ct) return <FileText className="size-4" />;
          if (/^(text\/|application\/(json|xml|x-yaml|toml|javascript|typescript))/i.test(ct)) {
            return <FileText className="size-4" />;
          }
          return <FileIcon className="size-4" />;
        })()}
      </span>
      <span
        className={cn(
          "flex-1 truncate",
          isRoot && "font-semibold tracking-tight text-sidebar-foreground",
          node.implicit && !isRoot && "italic text-muted-foreground"
        )}
        title={
          node.implicit
            ? "Implied by file paths. Files inside sync to this location."
            : undefined
        }
      >
        {isRoot ? "All files" : node.name}
      </span>
      {uploading ? (
        <Loader2 className="size-3.5 animate-spin text-amber-500" />
      ) : pending ? (
        <Spinner className="size-3.5" />
      ) : null}
    </button>
  );
}
