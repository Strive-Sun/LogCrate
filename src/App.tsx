import { useCallback, useEffect, useRef, useState } from 'react';
import { getCurrentWebview } from '@tauri-apps/api/webview';
import { api, isTauri } from './api';
import type {
  AppUpdateInfo,
  AppUpdateProgress,
  DroppedFileInfo,
  NewLogItem,
  OpenSessionResult,
  TreeNode,
} from './api';
import { TopBar } from './components/TopBar';
import { UpdateDialog } from './components/UpdateDialog';
import { ConfirmDialog } from './components/ConfirmDialog';
import { DirTree } from './components/DirTree';
import { LogContent } from './components/LogContent';
import { EmptyState } from './components/EmptyState';
import {
  classifyUpdateCheck,
  errorMessage,
  loadAutoCheck,
  loadSkippedVersion,
  saveAutoCheck,
  saveSkippedVersion,
  type UpdateStatus,
} from './util/update';
import {
  applyDirectoryChanges,
  findTreeNode,
  passesDirectoryFilter,
  revealDirectoryChain,
  removedDirectoryNodes,
} from './util/directoryTree';
import { planFileDrop, singleDroppedPath } from './util/fileDrop';

function flattenNodes(nodes: readonly TreeNode[]): TreeNode[] {
  return nodes.flatMap((node) => [node, ...flattenNodes(node.children ?? [])]);
}

interface ConfirmationRequest {
  title: string;
  message: string;
  confirmLabel: string;
  action: () => Promise<void>;
}

export function App() {
  const [theme, setTheme] = useState<'dark' | 'light'>('light');
  const [appVersion, setAppVersion] = useState('…');
  const [autoCheckUpdates, setAutoCheckUpdates] = useState(() => loadAutoCheck(localStorage));
  const [skippedVersion, setSkippedVersion] = useState(() => loadSkippedVersion(localStorage));
  const [updateStatus, setUpdateStatus] = useState<UpdateStatus>('idle');
  const [updateInfo, setUpdateInfo] = useState<AppUpdateInfo | null>(null);
  const [updateProgress, setUpdateProgress] = useState<AppUpdateProgress | null>(null);
  const [updateError, setUpdateError] = useState<string | null>(null);
  const [updatePromptOpen, setUpdatePromptOpen] = useState(false);
  const [confirmation, setConfirmation] = useState<ConfirmationRequest | null>(null);
  const confirmationRef = useRef<ConfirmationRequest | null>(null);
  const updatePromptOpenRef = useRef(false);
  const dropBusy = useRef(false);
  const updateTaskRunning = useRef(false);
  const autoCheckStarted = useRef(false);
  const [tree, setTree] = useState<TreeNode[]>([]);
  const treeRef = useRef<TreeNode[]>([]);
  const [newItems, setNewItems] = useState<NewLogItem[]>([]);
  // 徽章数字直接由未读列表长度派生,保证徽章与列表始终一致
  const count = newItems.length;
  // 未读项 id 集合(id 即文件路径),用于左树高亮;不依赖后端 unread 标记
  const unreadIds = new Set(newItems.map((it) => it.id));
  const seen = useRef<Set<string>>(new Set());
  // 打开请求序号:防止并发打开时旧请求覆盖新请求的视图状态
  const openSeq = useRef(0);
  // activeKey 的实时镜像:供 rename/delete 在 await 之后读取当前值(避免闭包捕获过时值)
  const activeKeyRef = useRef<string | null>(null);

  // 当前选中的压缩包(用于左侧树高亮)与当前查看的条目 key
  const [selectedArchive, setSelectedArchive] = useState<string | null>(null);
  const selectedArchiveRef = useRef<string | null>(null);
  const [revealedTarget, setRevealedTarget] = useState<{
    path: string;
    directories: string[];
  } | null>(null);
  const [session, setSession] = useState<OpenSessionResult | null>(null);
  const [activeKey, setActiveKey] = useState<string | null>(null);

  // 后缀筛选
  const [filter, setFilter] = useState<string[]>(['.log', '.txt', '.out']);
  const [showAll, setShowAll] = useState(false);
  // 用户一旦本地修改筛选,忽略启动时异步返回的旧配置,避免覆盖新值
  const filterEdited = useRef(false);

  // 左栏宽度(可拖动调整),持久化到 localStorage
  const [treeWidth, setTreeWidth] = useState<number>(() => {
    const saved = Number(localStorage.getItem('logpeek.treeWidth'));
    return saved >= 160 && saved <= 720 ? saved : 300;
  });

  const startResize = useCallback(
    (e: React.MouseEvent) => {
      e.preventDefault();
      const startX = e.clientX;
      const startW = treeWidth;
      const onMove = (ev: MouseEvent) => {
        const w = Math.min(720, Math.max(160, startW + ev.clientX - startX));
        setTreeWidth(w);
      };
      const onUp = () => {
        document.removeEventListener('mousemove', onMove);
        document.removeEventListener('mouseup', onUp);
        document.body.classList.remove('resizing');
      };
      document.addEventListener('mousemove', onMove);
      document.addEventListener('mouseup', onUp);
      document.body.classList.add('resizing');
    },
    [treeWidth],
  );

  const checkForUpdates = useCallback(
    async (automatic: boolean) => {
      if (updateTaskRunning.current) return;
      updateTaskRunning.current = true;
      setUpdateStatus('checking');
      setUpdateError(null);
      setUpdateProgress(null);
      try {
        const update = await api.checkForUpdate();
        const outcome = classifyUpdateCheck(update, automatic, skippedVersion);
        if (outcome === 'up-to-date') {
          setUpdateInfo(null);
          setUpdateStatus(automatic ? 'idle' : 'up-to-date');
          return;
        }
        if (outcome === 'skipped') {
          await api.discardPendingUpdate();
          setUpdateInfo(null);
          setUpdateStatus('idle');
          return;
        }
        if (!update) return;
        setUpdateInfo(update);
        setUpdateStatus('available');
        if (automatic) setUpdatePromptOpen(true);
      } catch (error) {
        setUpdateInfo(null);
        if (automatic) {
          setUpdateStatus('idle');
        } else {
          setUpdateError(errorMessage(error));
          setUpdateStatus('error');
        }
      } finally {
        updateTaskRunning.current = false;
      }
    },
    [skippedVersion],
  );

  const changeAutoCheckUpdates = useCallback((enabled: boolean) => {
    setAutoCheckUpdates(enabled);
    saveAutoCheck(localStorage, enabled);
  }, []);

  const skipUpdate = useCallback(() => {
    if (updateInfo) {
      saveSkippedVersion(localStorage, updateInfo.version);
      setSkippedVersion(updateInfo.version);
    }
    setUpdatePromptOpen(false);
    setUpdateInfo(null);
    setUpdateStatus('idle');
    setUpdateProgress(null);
    void api.discardPendingUpdate().catch(() => undefined);
  }, [updateInfo]);

  const downloadUpdate = useCallback(async () => {
    if (updateTaskRunning.current || !updateInfo) return;
    updateTaskRunning.current = true;
    setUpdatePromptOpen(false);
    setUpdateError(null);
    setUpdateStatus('downloading');
    setUpdateProgress({ phase: 'downloading', downloadedBytes: 0 });
    try {
      await api.downloadAndInstallUpdate((progress) => {
        setUpdateProgress(progress);
        setUpdateStatus(progress.phase);
      });
      setUpdateStatus('installed');
    } catch (error) {
      setUpdateError(errorMessage(error));
      setUpdateStatus('error');
    } finally {
      updateTaskRunning.current = false;
    }
  }, [updateInfo]);

  useEffect(() => {
    api
      .getAppVersion()
      .then(setAppVersion)
      .catch(() => setAppVersion('未知'));
  }, []);

  useEffect(() => {
    if (autoCheckStarted.current) return;
    autoCheckStarted.current = true;
    if (autoCheckUpdates) void checkForUpdates(true);
  }, [autoCheckUpdates, checkForUpdates]);

  useEffect(() => {
    localStorage.setItem('logpeek.treeWidth', String(treeWidth));
  }, [treeWidth]);

  useEffect(() => {
    document.documentElement.dataset.theme = theme;
  }, [theme]);

  // 保持 activeKey 镜像与状态同步,供异步回调读取最新值
  useEffect(() => {
    activeKeyRef.current = activeKey;
  }, [activeKey]);

  useEffect(() => {
    selectedArchiveRef.current = selectedArchive;
  }, [selectedArchive]);

  useEffect(() => {
    confirmationRef.current = confirmation;
  }, [confirmation]);

  useEffect(() => {
    updatePromptOpenRef.current = updatePromptOpen;
  }, [updatePromptOpen]);

  // 禁用 WebView 默认右键菜单(刷新/打印/检查等,对本应用无意义)
  useEffect(() => {
    const onCtx = (e: MouseEvent) => e.preventDefault();
    document.addEventListener('contextmenu', onCtx);
    return () => document.removeEventListener('contextmenu', onCtx);
  }, []);

  // 清空当前查看视图，并使任何进行中的打开请求失效。
  const resetView = useCallback(() => {
    openSeq.current++;
    setActiveKey(null);
    setSession(null);
    setSelectedArchive(null);
  }, []);

  const refreshTree = useCallback(async () => {
    const nodes = await api.listWatchDirs();
    treeRef.current = nodes;
    setTree(nodes);
    return nodes;
  }, []);

  useEffect(() => {
    refreshTree();
    api.newLogItems().then(setNewItems);
    // 启动时同步后端持久化的后缀筛选,避免前后端筛选分叉(通知与可见树不一致)
    api.getFilter().then(([suffixes, showAllCfg]) => {
      // 若用户在响应返回前已修改筛选,则不用旧配置覆盖
      if (filterEdited.current) return;
      setFilter(suffixes);
      setShowAll(showAllCfg);
    });
    // 订阅后端到达事件
    const unsub = api.subscribeNewLogs((item) => {
      // 已读过的项不再加回;同一 id 只保留一条,避免重复事件导致计数虚高
      if (seen.current.has(item.id)) return;
      setNewItems((prev) => (prev.some((p) => p.id === item.id) ? prev : [item, ...prev]));
    });
    const unsubChanges = api.subscribeDirectoryChanges((batch) => {
      const before = treeRef.current;
      const after = applyDirectoryChanges(before, batch);
      treeRef.current = after;
      setTree(after);

      const removed = removedDirectoryNodes(before, after, batch.watchDir);
      if (removed.length === 0) return;
      removed.forEach((node) => {
        if (node.kind === 'dir') void api.collapseDirectory(node.path ?? node.id);
      });
      const removedTree = flattenNodes(removed);
      const removedIds = new Set(removedTree.map((node) => node.id));
      setNewItems((items) =>
        items.filter((item) => {
          if (!removedIds.has(item.id)) return true;
          seen.current.add(item.id);
          return false;
        }),
      );
      const active = activeKeyRef.current;
      const selected = selectedArchiveRef.current;
      if (
        removedTree.some(
          (node) =>
            active === node.name ||
            active === node.id ||
            active?.startsWith(node.name + '::') ||
            active?.startsWith(node.id + '::') ||
            selected === node.id,
        )
      ) {
        resetView();
      }
    });
    return () => {
      unsub();
      unsubChanges();
    };
  }, [refreshTree, resetView]);

  const addDir = useCallback(async () => {
    const ok = await api.addWatchDir();
    if (ok) refreshTree();
  }, [refreshTree]);

  const loadDirectory = useCallback(async (path: string) => {
    const children = await api.expandDirectory(path);
    const next = applyDirectoryChanges(treeRef.current, {
      watchDir: path,
      changes: [{ type: 'rescan', nodes: children }],
    });
    treeRef.current = next;
    setTree(next);
  }, []);

  const expandDirectory = useCallback(
    async (node: TreeNode) => {
      await loadDirectory(node.path ?? node.id);
    },
    [loadDirectory],
  );

  const collapseDirectory = useCallback((node: TreeNode) => {
    void api.collapseDirectory(node.path ?? node.id);
  }, []);

  const passesFilter = useCallback(
    (node: { name: string; kind: string; isLog?: boolean }) => {
      return passesDirectoryFilter(node, filter, showAll);
    },
    [filter, showAll],
  );

  const markSeen = useCallback((id: string) => {
    seen.current.add(id);
    setNewItems((items) => items.filter((it) => it.id !== id));
  }, []);

  const openEntry = useCallback(
    async (entryKey: string, unreadId?: string) => {
      // 请求令牌:仅最新一次打开可提交状态,避免旧请求的成功/失败覆盖新请求
      const token = ++openSeq.current;
      setActiveKey(entryKey);
      setSession(null);
      try {
        const s = await api.openLogSession(entryKey);
        // 点击通知即视为已读,与是否被更晚请求取代无关
        if (unreadId) markSeen(unreadId);
        if (token !== openSeq.current) return; // 已被更晚的打开取代,丢弃
        setSession(s);
      } catch (e) {
        // 失效的新到达项从通知列表移除,防止反复点开报错(与令牌无关,始终执行)
        if (unreadId) markSeen(unreadId);
        if (token !== openSeq.current) return; // 不覆盖更晚请求的状态
        // 打开失败(如文件已被重命名/删除):清空视图状态,避免卡在"打开中…"
        setSession(null);
        setActiveKey(null);
        alert('无法打开:' + String(e));
      }
    },
    [markSeen],
  );

  const revealNewItem = useCallback(
    async (item: NewLogItem, options?: { openFile?: boolean }) => {
      const directories = revealDirectoryChain(treeRef.current, item.id);
      if (directories.length === 0) {
        markSeen(item.id);
        setRevealedTarget(null);
        alert('无法定位：文件不在当前监控目录中或已经失效。');
        return;
      }

      try {
        for (const directory of directories) await loadDirectory(directory);
      } catch {
        markSeen(item.id);
        setRevealedTarget(null);
        alert('无法定位：文件所在目录已被移动、删除或无法读取。');
        return;
      }

      if (!findTreeNode(treeRef.current, item.id)) {
        markSeen(item.id);
        setRevealedTarget(null);
        alert('无法定位：文件已被移动或删除。');
        return;
      }

      setRevealedTarget({ path: item.id, directories });
      if (item.kind === 'file') {
        if (options?.openFile === false) markSeen(item.id);
        else await openEntry(item.id, item.id);
      } else {
        setSelectedArchive(item.id);
        markSeen(item.id);
      }
    },
    [loadDirectory, markSeen, openEntry],
  );

  const handleDroppedPaths = useCallback(
    async (paths: readonly string[]) => {
      if (dropBusy.current) {
        alert('正在处理另一个拖入文件，请稍候。');
        return;
      }
      if (confirmationRef.current || updatePromptOpenRef.current) {
        alert('请先完成当前弹窗操作，再拖入文件。');
        return;
      }

      dropBusy.current = true;
      try {
        const path = singleDroppedPath(paths);
        const info: DroppedFileInfo = await api.inspectDroppedFile(path);
        const plan = planFileDrop(info);

        // 日志查看与监控添加互不依赖，检查通过后立即启动现有打开流程。
        if (plan.openPath) void openEntry(plan.openPath);

        if (plan.watchPathToAdd) {
          await api.addWatchPath(plan.watchPathToAdd);
          await refreshTree();
        }

        if (plan.locateInTree && info.kind !== 'directory') {
          await revealNewItem(
            {
              id: info.path,
              name: info.name,
              kind: info.kind,
              source: info.watchPath,
              age: 'now',
            },
            { openFile: false },
          );
        }
      } catch (error) {
        alert('拖入处理失败：' + String(error));
      } finally {
        dropBusy.current = false;
      }
    },
    [openEntry, refreshTree, revealNewItem],
  );

  useEffect(() => {
    if (!isTauri) return;
    let disposed = false;
    let unlisten: (() => void) | undefined;
    void getCurrentWebview()
      .onDragDropEvent((event) => {
        if (event.payload.type === 'drop') void handleDroppedPaths(event.payload.paths);
      })
      .then((stop) => {
        if (disposed) stop();
        else unlisten = stop;
      })
      .catch((error) => alert('无法启用文件拖放：' + String(error)));
    return () => {
      disposed = true;
      unlisten?.();
    };
  }, [handleDroppedPaths]);

  const finishReveal = useCallback(() => setRevealedTarget(null), []);

  const markAllRead = useCallback(() => {
    // 记住已读,避免重复事件把它们重新加回列表
    setNewItems((items) => {
      items.forEach((it) => seen.current.add(it.id));
      return [];
    });
  }, []);

  const renameNode = useCallback(
    async (node: TreeNode, newName: string) => {
      const path = node.path ?? node.id;
      try {
        if (node.kind === 'dir') await api.renameWatchDir(path, newName);
        else await api.renameFile(path, newName);
        // 旧路径已失效:移除指向旧路径的通知项,避免点开报错
        markSeen(node.id);
        // 若正在查看被重命名的项,其会话已指向旧路径,重置视图。
        // 读取镜像的当前值(await 期间用户可能已切换查看目标)。
        // 裸文件 activeKey 为文件名;压缩包内条目 activeKey 为 `名称::条目`。
        const cur = activeKeyRef.current;
        if (
          node.kind !== 'dir' &&
          (cur === node.name ||
            cur === node.id ||
            cur?.startsWith(node.name + '::') ||
            cur?.startsWith(node.id + '::'))
        ) {
          resetView();
        }
        refreshTree();
      } catch (e) {
        alert('重命名失败:' + String(e));
      }
    },
    [refreshTree, markSeen, resetView],
  );

  const openPath = useCallback(async (node: TreeNode) => {
    try {
      await api.openPath(node.path ?? node.id);
    } catch (e) {
      alert('打开失败:' + String(e));
    }
  }, []);

  const removeWatch = useCallback(
    async (node: TreeNode) => {
      const path = node.path ?? node.id;
      try {
        await api.removeWatchDir(path);
        refreshTree();
      } catch (e) {
        alert('移除失败:' + String(e));
      }
    },
    [refreshTree],
  );

  const deleteDir = useCallback(
    (node: TreeNode) => {
      const path = node.path ?? node.id;
      setConfirmation({
        title: `确定删除整个目录「${node.name}」吗？`,
        message: '目录及其全部内容将被移到系统回收站，并停止监控。',
        confirmLabel: '删除目录',
        action: async () => {
          try {
            await api.deleteWatchDir(path);
            resetView();
            // 移除该目录下所有失效的通知项(id 为完整路径,以目录路径为前缀)
            const prefixes = [path + '/', path + '\\'];
            setNewItems((items) =>
              items.filter((it) => {
                const stale = it.id === path || prefixes.some((p) => it.id.startsWith(p));
                if (stale) seen.current.add(it.id);
                return !stale;
              }),
            );
            refreshTree();
          } catch (e) {
            alert('删除失败:' + String(e));
          }
        },
      });
    },
    [refreshTree, resetView],
  );

  const deleteFile = useCallback(
    (node: TreeNode) => {
      const target = node.path ?? node.id;
      setConfirmation({
        title: `确定删除「${node.name}」吗？`,
        message: '文件将被移到系统回收站。',
        confirmLabel: '删除文件',
        action: async () => {
          try {
            await api.deleteFile(target);
            // 若当前查看的正是被删文件(或被删压缩包内的条目),清空视图。
            // 读取镜像的当前值(await 期间用户可能已切换查看目标)。
            const cur = activeKeyRef.current;
            if (
              cur === node.name ||
              cur === node.id ||
              cur?.startsWith(node.name + '::') ||
              cur?.startsWith(node.id + '::')
            ) {
              resetView();
            }
            markSeen(node.id);
            refreshTree();
          } catch (e) {
            alert('删除失败:' + String(e));
          }
        },
      });
    },
    [markSeen, refreshTree, resetView],
  );

  const hasDirs = tree.length > 0;

  return (
    <div className="app">
      <TopBar
        theme={theme}
        onToggleTheme={() => setTheme((t) => (t === 'dark' ? 'light' : 'dark'))}
        count={count}
        newItems={newItems}
        onOpenItem={(item) => void revealNewItem(item)}
        onMarkAll={markAllRead}
        appVersion={appVersion}
        autoCheckUpdates={autoCheckUpdates}
        updateStatus={updateStatus}
        updateInfo={updateInfo}
        updateProgress={updateProgress}
        updateError={updateError}
        onAutoCheckUpdatesChange={changeAutoCheckUpdates}
        onCheckForUpdates={() => void checkForUpdates(false)}
        onSkipUpdate={skipUpdate}
        onDownloadUpdate={() => void downloadUpdate()}
      />

      {updatePromptOpen && updateInfo && (
        <UpdateDialog
          update={updateInfo}
          onSkip={skipUpdate}
          onDownload={() => void downloadUpdate()}
        />
      )}

      {confirmation && (
        <ConfirmDialog
          title={confirmation.title}
          message={confirmation.message}
          confirmLabel={confirmation.confirmLabel}
          onCancel={() => setConfirmation(null)}
          onConfirm={() => {
            const action = confirmation.action;
            setConfirmation(null);
            void action();
          }}
        />
      )}

      <div className="cols">
        <DirTree
          nodes={tree}
          activeKey={activeKey}
          selectedArchive={selectedArchive}
          revealPath={revealedTarget?.path ?? null}
          revealDirectories={revealedTarget?.directories ?? []}
          onRevealComplete={finishReveal}
          width={treeWidth}
          unreadIds={unreadIds}
          filter={filter}
          showAll={showAll}
          passesFilter={passesFilter}
          onFilterChange={(f) => {
            filterEdited.current = true;
            setFilter(f);
            void api.setFilter(f, showAll);
          }}
          onShowAllChange={(v) => {
            filterEdited.current = true;
            setShowAll(v);
            void api.setFilter(filter, v);
          }}
          onAddDir={addDir}
          onExpandDirectory={expandDirectory}
          onCollapseDirectory={collapseDirectory}
          onSelectArchive={(name, id) => {
            setSelectedArchive(name);
            if (id) markSeen(id);
          }}
          onOpenFile={(name, id) => openEntry(name, id)}
          onRename={renameNode}
          onDelete={deleteFile}
          onOpenPath={openPath}
          onRemoveWatch={removeWatch}
          onDeleteDir={deleteDir}
        />
        <div className="col-resizer" onMouseDown={startResize} />

        {hasDirs || session || activeKey ? (
          <LogContent session={session} activeKey={activeKey} />
        ) : (
          <EmptyState onAddDir={addDir} />
        )}
      </div>
    </div>
  );
}
