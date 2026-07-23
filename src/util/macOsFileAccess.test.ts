import assert from 'node:assert/strict';
import test from 'node:test';
import {
  markMacOsFileAccessOnboardingSeen,
  shouldShowMacOsFileAccessOnboarding,
} from './macOsFileAccess';

test('macOS file access onboarding is shown once per version', () => {
  const values = new Map<string, string>();
  const storage = {
    getItem: (key: string) => values.get(key) ?? null,
    setItem: (key: string, value: string) => values.set(key, value),
  };
  assert.equal(shouldShowMacOsFileAccessOnboarding(storage, 1), true);
  markMacOsFileAccessOnboardingSeen(storage, 1);
  assert.equal(shouldShowMacOsFileAccessOnboarding(storage, 1), false);
  assert.equal(shouldShowMacOsFileAccessOnboarding(storage, 2), true);
});
