import type { FileMetadata } from "@/module_bindings/types";

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
