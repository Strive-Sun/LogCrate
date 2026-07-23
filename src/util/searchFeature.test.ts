import assert from 'node:assert/strict';
import test from 'node:test';
import { nextSearchOpen, searchRestartNoticeKey } from './searchFeature';

test('search entry toggles between monitoring and search views only when enabled', () => {
  const enabled = { currentEnabled: true, nextLaunchEnabled: true };
  assert.equal(nextSearchOpen(false, enabled), true);
  assert.equal(nextSearchOpen(true, enabled), false);
  assert.equal(nextSearchOpen(false, { currentEnabled: false, nextLaunchEnabled: true }), false);
  assert.equal(nextSearchOpen(false, null), false);
});

test('each search preference change has a next-launch notice', () => {
  assert.equal(searchRestartNoticeKey(true), 'settings.searchEnabledNextLaunch');
  assert.equal(searchRestartNoticeKey(false), 'settings.searchDisabledNextLaunch');
});
