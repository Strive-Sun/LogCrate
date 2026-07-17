import type { DirectoryChangeBatch, TreeNode } from '../api/types';

function compareNodes(a: TreeNode, b: TreeNode): number {
  if (a.kind === 'dir' && b.kind !== 'dir') return -1;
  if (a.kind !== 'dir' && b.kind === 'dir') return 1;
  const byName = a.name.toLocaleLowerCase().localeCompare(b.name.toLocaleLowerCase());
  return byName || a.id.localeCompare(b.id);
}

function prepareNode(node: TreeNode, directory: TreeNode, previous?: TreeNode): TreeNode {
  return {
    ...previous,
    ...node,
    id: node.path ?? node.id,
    watchDir: directory.name,
    children: node.children ?? previous?.children,
  };
}

/** 将一个后端变化批次应用到单个监控目录，未受影响目录保持引用不变。 */
export function applyDirectoryChanges(
  tree: readonly TreeNode[],
  batch: DirectoryChangeBatch,
): TreeNode[] {
  function update(directory: TreeNode): TreeNode {
    if (directory.id !== batch.watchDir && directory.path !== batch.watchDir) {
      if (!directory.children) return directory;
      let childChanged = false;
      const children = directory.children.map((child) => {
        if (child.kind !== 'dir') return child;
        const next = update(child);
        if (next !== child) childChanged = true;
        return next;
      });
      return childChanged ? { ...directory, children } : directory;
    }
    let children = [...(directory.children ?? [])];

    for (const change of batch.changes) {
      if (change.type === 'rescan') {
        const previous = new Map(children.map((node) => [node.id, node]));
        children = change.nodes.map((node) =>
          prepareNode(node, directory, previous.get(node.path ?? node.id)),
        );
        continue;
      }
      if (change.type === 'remove') {
        children = children.filter((node) => node.id !== change.path && node.path !== change.path);
        continue;
      }
      if (change.type === 'rename') {
        children = children.filter(
          (node) => node.id !== change.oldPath && node.path !== change.oldPath,
        );
      }
      const incoming = change.node;
      const id = incoming.path ?? incoming.id;
      const index = children.findIndex((node) => node.id === id || node.path === id);
      const node = prepareNode(incoming, directory, index >= 0 ? children[index] : undefined);
      if (index >= 0) children[index] = node;
      else children.push(node);
    }

    children.sort(compareNodes);
    return { ...directory, children };
  }

  return tree.map(update);
}

export function findTreeNode(tree: readonly TreeNode[], path: string): TreeNode | undefined {
  for (const node of tree) {
    if (sameFilePath(node.id, path) || (node.path ? sameFilePath(node.path, path) : false)) {
      return node;
    }
    const nested = node.children ? findTreeNode(node.children, path) : undefined;
    if (nested) return nested;
  }
  return undefined;
}

interface NormalizedPath {
  value: string;
  comparable: string;
  windows: boolean;
}

function normalizePath(path: string): NormalizedPath {
  const windows = /^[a-zA-Z]:[\\/]/.test(path);
  let value = path.replace(/\\/g, '/').replace(/\/{2,}/g, '/');
  const isRoot = value === '/' || /^[a-zA-Z]:\/$/.test(value);
  if (!isRoot) value = value.replace(/\/$/, '');
  return {
    value,
    comparable: windows ? value.toLocaleLowerCase() : value,
    windows,
  };
}

export function sameFilePath(left: string, right: string): boolean {
  const a = normalizePath(left);
  const b = normalizePath(right);
  return a.windows === b.windows && a.comparable === b.comparable;
}

function containsPath(root: NormalizedPath, target: NormalizedPath): boolean {
  if (root.windows !== target.windows) return false;
  if (root.comparable === target.comparable) return true;
  const prefix = root.comparable.endsWith('/') ? root.comparable : root.comparable + '/';
  return target.comparable.startsWith(prefix);
}

export function isPathInsideDirectory(path: string, directory: string): boolean {
  const target = normalizePath(path);
  const root = normalizePath(directory);
  return root.comparable !== target.comparable && containsPath(root, target);
}

/**
 * 返回定位目标文件时需要依次读取的目录链，包含监控根和目标父目录。
 * 只做路径计算，不访问磁盘；重叠监控根选择最具体的一条。
 */
export function revealDirectoryChain(tree: readonly TreeNode[], targetPath: string): string[] {
  const target = normalizePath(targetPath);
  const roots = tree
    .filter((node) => node.kind === 'dir' && node.watchRoot)
    .map((node) => ({ node, path: normalizePath(node.path ?? node.id) }))
    .filter(({ path }) => containsPath(path, target))
    .sort((a, b) => b.path.comparable.length - a.path.comparable.length);
  const match = roots[0];
  if (!match || match.path.comparable === target.comparable) return [];

  const relative = target.value.slice(match.path.value.length).replace(/^\//, '');
  const parts = relative.split('/').filter(Boolean);
  parts.pop();

  const rootPath = match.node.path ?? match.node.id;
  const separator = match.path.windows ? '\\' : '/';
  const chain = [rootPath];
  let current = rootPath;
  for (const part of parts) {
    current += current.endsWith('\\') || current.endsWith('/') ? part : separator + part;
    chain.push(current);
  }
  return chain;
}

/** 返回一个批次应用后从受影响目录消失的旧顶层节点。 */
export function removedDirectoryNodes(
  before: readonly TreeNode[],
  after: readonly TreeNode[],
  watchDir: string,
): TreeNode[] {
  const oldDir = findTreeNode(before, watchDir);
  const newDir = findTreeNode(after, watchDir);
  const remaining = new Set((newDir?.children ?? []).map((node) => node.id));
  return (oldDir?.children ?? []).filter((node) => !remaining.has(node.id));
}

export function passesDirectoryFilter(
  node: { name: string; kind: string },
  suffixes: readonly string[],
  showAll: boolean,
): boolean {
  if (node.kind === 'dir' || node.kind === 'archive' || showAll) return true;
  const lower = node.name.toLocaleLowerCase();
  return suffixes.some((suffix) => lower.endsWith(suffix.toLocaleLowerCase()));
}
