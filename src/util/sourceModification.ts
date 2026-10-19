export type SourceModifiedAction = 'ask' | 'reload' | 'ignore';

export const SOURCE_MODIFIED_ACTION_STORAGE_KEY = 'logcrate.sourceModifiedAction.v1';
export const DEFAULT_SOURCE_MODIFIED_ACTION: SourceModifiedAction = 'ask';

export function isSourceModifiedAction(value: unknown): value is SourceModifiedAction {
  return value === 'ask' || value === 'reload' || value === 'ignore';
}

export function loadSourceModifiedAction(storage: Pick<Storage, 'getItem'>): SourceModifiedAction {
  try {
    const value = storage.getItem(SOURCE_MODIFIED_ACTION_STORAGE_KEY);
    return isSourceModifiedAction(value) ? value : DEFAULT_SOURCE_MODIFIED_ACTION;
  } catch {
    return DEFAULT_SOURCE_MODIFIED_ACTION;
  }
}

export function saveSourceModifiedAction(
  storage: Pick<Storage, 'setItem'>,
  action: SourceModifiedAction,
): void {
  try {
    storage.setItem(SOURCE_MODIFIED_ACTION_STORAGE_KEY, action);
  } catch {
    // WebView 存储不可用时仍保留当前进程内选择。
  }
}

export type SourceRevisionDecision = 'prompt' | 'remember' | 'reload' | 'none';

/** 修订已变化后按偏好决定处理方式；自动重载失败过的同一修订不再重试。 */
export function decideSourceRevisionChange(
  action: SourceModifiedAction,
  revision: string,
  failedRevision?: string,
): SourceRevisionDecision {
  if (action === 'ask') return 'prompt';
  if (action === 'ignore') return 'remember';
  return revision === failedRevision ? 'none' : 'reload';
}

export type SourceReloadKind = 'auto' | 'explicit';

interface ReloadSlot {
  path: string;
  running: Promise<void>;
  pending: (() => Promise<void>) | null;
}

/**
 * 同一磁盘源的重新加载串行器：任意时刻每个源最多一个在途任务。
 * 在途期间的自动请求直接丢弃；提示框或手动请求合并为一次待办，在当前任务结束后执行。
 */
export function createSourceReloadQueue(samePath: (left: string, right: string) => boolean) {
  const slots: ReloadSlot[] = [];

  const start = (path: string, task: () => Promise<void>): Promise<void> => {
    const slot: ReloadSlot = { path, running: Promise.resolve(), pending: null };
    slots.push(slot);
    const drain = async () => {
      let next: (() => Promise<void>) | null = task;
      while (next) {
        const current = next;
        slot.pending = null;
        await current().catch(() => undefined);
        next = slot.pending;
      }
      slots.splice(slots.indexOf(slot), 1);
    };
    slot.running = drain();
    return slot.running;
  };

  return {
    run(path: string, kind: SourceReloadKind, task: () => Promise<void>): Promise<void> {
      const slot = slots.find((item) => samePath(item.path, path));
      if (!slot) return start(path, task);
      if (kind === 'explicit') slot.pending = task;
      return slot.running;
    },
    isRunning(path: string): boolean {
      return slots.some((item) => samePath(item.path, path));
    },
  };
}
