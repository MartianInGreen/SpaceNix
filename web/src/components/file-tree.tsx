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
import { Spinner } from "@/components/common";
import type { TreeNode } from "@/components/file-tree-utils";

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
