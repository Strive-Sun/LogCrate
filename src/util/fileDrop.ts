import type { DroppedFileInfo } from '../api/types';

export type FileDropPlan = {
  openPath: string | null;
  watchPathToAdd: string | null;
  locateInTree: boolean;
};

export function singleDroppedPath(paths: readonly string[]): string {
  if (paths.length !== 1) {
    throw new Error('当前仅支持一次拖入一个文件或文件夹，多路径拖入将在后续版本支持。');
  }
  return paths[0];
}

export function planFileDrop(info: DroppedFileInfo): FileDropPlan {
  return {
    openPath: info.isLog ? info.path : null,
    watchPathToAdd: info.alreadyMonitored ? null : info.watchPath,
    locateInTree: info.kind === 'archive' || info.isLog,
  };
}
