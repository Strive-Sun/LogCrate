import type { FileSearchFeatureState } from '../api';

export function nextSearchOpen(
  currentOpen: boolean,
  feature: FileSearchFeatureState | null,
): boolean {
  return feature?.currentEnabled ? !currentOpen : false;
}

export function searchRestartNoticeKey(
  enabled: boolean,
): 'settings.searchEnabledNextLaunch' | 'settings.searchDisabledNextLaunch' {
  return enabled ? 'settings.searchEnabledNextLaunch' : 'settings.searchDisabledNextLaunch';
}
