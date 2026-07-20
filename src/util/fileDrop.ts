import type { DroppedFileInfo } from '../api/types';

export type FileDropPlan = {
  openPath: string | null;
  watchPathToAdd: string | null;
  locateInTree: boolean;
};

export function singleDroppedPath(paths: readonly string[]): string {
  if (paths.length !== 1) {
    throw new Error('fileDrop.single');
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
