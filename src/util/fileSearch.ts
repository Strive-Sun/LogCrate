import type {
  DroppedFileInfo,
  FileSearchProviderStatus,
  FileSearchResult,
  FileSearchStatus,
} from '../api';

export type SearchOpenAction = 'archive' | 'log' | 'reveal';

export function planSearchOpen(info: DroppedFileInfo): SearchOpenAction {
  if (info.kind === 'archive') return 'archive';
  if (info.isLog) return 'log';
  return 'reveal';
}

export function shouldApplySearchResponse(requestGeneration: number, currentGeneration: number) {
  return requestGeneration === currentGeneration;
}

export function searchProgressRefreshKey(status: FileSearchStatus | null): string {
  if (!status) return 'loading';
  const indexedBucket = Math.floor(status.indexedFiles / 100_000);
  const providers = status.providers.map((item) => `${item.root}:${item.phase}`).join('|');
  return `${status.phase}:${indexedBucket}:${providers}`;
}

export function providerElapsedSeconds(provider: FileSearchProviderStatus): {
  total: string;
  stage: string;
} {
  return {
    total: ((provider.elapsedMs ?? 0) / 1000).toFixed(1),
    stage: ((provider.stageElapsedMs ?? 0) / 1000).toFixed(1),
  };
}

export function searchPreparation(status: FileSearchStatus):
  | {
      roots: string;
      stage: string;
    }
  | undefined {
  if (status.phase !== 'scanning') return undefined;
  const active = status.providers.filter((provider) =>
    ['scanning', 'merging'].includes(provider.phase),
  );
  const discovered = active.reduce(
    (total, provider) => total + (provider.discoveredRecords ?? 0),
    0,
  );
  if (active.length === 0 || discovered > 0) return undefined;
  const stages = [...new Set(active.map((provider) => provider.stage ?? provider.phase))];
  return {
    roots: active.map((provider) => provider.root).join('、'),
    stage: stages.length === 1 ? stages[0] : 'multiple',
  };
}

export function mergeSearchResults(
  current: readonly FileSearchResult[],
  incoming: readonly FileSearchResult[],
  append: boolean,
  maximum: number,
): FileSearchResult[] {
  const source = append ? [...current, ...incoming] : [...incoming];
  const seen = new Set<string>();
  return source
    .filter((item) => {
      const key = navigatorPlatformPathKey(item.path);
      if (seen.has(key)) return false;
      seen.add(key);
      return true;
    })
    .slice(0, Math.max(0, maximum));
}

function navigatorPlatformPathKey(path: string): string {
  return /^[a-z]:[/\\]/i.test(path) ? path.replace(/\//g, '\\').toLocaleLowerCase() : path;
}
