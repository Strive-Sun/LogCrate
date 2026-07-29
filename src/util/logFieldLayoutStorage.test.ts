import assert from 'node:assert/strict';
import test from 'node:test';
import {
  clearSavedLogFieldLayout,
  loadLogFieldLayout,
  LOG_FIELD_LAYOUT_STORAGE_KEY,
  normalizeLogFileIdentity,
  persistLogFieldLayout,
  savedLayoutFingerprintMatches,
  type StoredLogFieldLayout,
} from './logFieldLayoutStorage';

class MemoryStorage {
  values = new Map<string, string>();
  writes = 0;
  failWrites = false;

  getItem(key: string) {
    return this.values.get(key) ?? null;
  }

  setItem(key: string, value: string) {
    if (this.failWrites) throw new Error('storage unavailable');
    this.writes += 1;
    this.values.set(key, value);
  }
}

function layout(
  source: StoredLogFieldLayout['source'] = 'automatic',
  fingerprint = 'bracketed-v1',
): StoredLogFieldLayout {
  return {
    source,
    fingerprint,
    encodingHint: 'UTF-8',
    fields: [
      { id: 'time', name: '时间', type: 'time', start: 0, end: 36 },
      { id: 'level', name: '级别', type: 'level', start: 37, end: 43 },
      { id: 'message', name: '正文', type: 'text', start: 44, end: null },
    ],
  };
}

test('normalizes bare files and archive entries into independent identities', () => {
  assert.equal(normalizeLogFileIdentity('C:\\Logs\\old\\..\\App.log'), 'file:c:/logs/app.log');
  assert.equal(
    normalizeLogFileIdentity('C:\\Logs\\bundle.zip::logs\\today\\app.log'),
    'archive:c:/logs/bundle.zip::logs/today/app.log',
  );
  assert.notEqual(
    normalizeLogFileIdentity('/logs/Main.log'),
    normalizeLogFileIdentity('/logs/main.log'),
  );
  assert.equal(
    normalizeLogFileIdentity('\\\\Server\\Share\\Logs\\App.log'),
    'file://server/share/logs/app.log',
  );
});

test('round-trips stable automatic layouts without persisting filter state', () => {
  const storage = new MemoryStorage();
  assert.equal(
    persistLogFieldLayout(storage, 'C:\\Logs\\App.log', layout(), 'stableAutomatic', 10),
    true,
  );
  assert.deepEqual(loadLogFieldLayout(storage, 'c:/logs/app.log', 11), layout());
  const raw = storage.getItem(LOG_FIELD_LAYOUT_STORAGE_KEY)!;
  assert.equal(raw.includes('filter'), false);
  assert.equal(raw.includes('candidate'), false);
  assert.equal(raw.includes('lineNo'), false);
});

test('does not overwrite a stable record with editing, invalid, or mismatched-source states', () => {
  const storage = new MemoryStorage();
  persistLogFieldLayout(storage, '/logs/app.log', layout(), 'stableAutomatic', 1);
  const edited = layout('manual', 'manual-v2');
  assert.equal(
    persistLogFieldLayout(storage, '/logs/app.log', edited, 'boundaryDragging', 2),
    false,
  );
  assert.equal(persistLogFieldLayout(storage, '/logs/app.log', edited, 'nameEditing', 3), false);
  assert.equal(persistLogFieldLayout(storage, '/logs/app.log', edited, 'invalid', 4), false);
  assert.equal(
    persistLogFieldLayout(storage, '/logs/app.log', edited, 'stableAutomatic', 5),
    false,
  );
  assert.deepEqual(loadLogFieldLayout(storage, '/logs/app.log', 6), layout());

  assert.equal(
    persistLogFieldLayout(storage, '/logs/app.log', edited, 'boundaryDragCommitted', 7),
    true,
  );
  assert.deepEqual(loadLogFieldLayout(storage, '/logs/app.log', 8), edited);
});

test('persists each confirmed manual adjustment trigger', () => {
  const storage = new MemoryStorage();
  const triggers = [
    'boundaryDragCommitted',
    'nameCommitted',
    'typeChanged',
    'fieldSplit',
    'fieldMerged',
  ] as const;
  for (const [index, trigger] of triggers.entries()) {
    const candidate = layout('manual', trigger);
    assert.equal(
      persistLogFieldLayout(storage, `/logs/${trigger}.log`, candidate, trigger, index + 1),
      true,
    );
    assert.deepEqual(loadLogFieldLayout(storage, `/logs/${trigger}.log`, 100 + index), candidate);
  }
});

test('rejects corrupt layouts and safely ignores damaged or unknown stores', () => {
  const storage = new MemoryStorage();
  const duplicate = layout();
  duplicate.fields[1].id = duplicate.fields[0].id;
  assert.equal(
    persistLogFieldLayout(storage, '/logs/app.log', duplicate, 'stableAutomatic', 1),
    false,
  );

  storage.values.set(LOG_FIELD_LAYOUT_STORAGE_KEY, '{broken');
  assert.equal(loadLogFieldLayout(storage, '/logs/app.log'), null);
  storage.values.set(LOG_FIELD_LAYOUT_STORAGE_KEY, JSON.stringify({ version: 99, entries: [] }));
  assert.equal(loadLogFieldLayout(storage, '/logs/app.log'), null);
});

test('keeps layouts path-scoped for bare files and archive entries', () => {
  const storage = new MemoryStorage();
  persistLogFieldLayout(storage, '/logs/a.log', layout(), 'stableAutomatic', 1);
  persistLogFieldLayout(
    storage,
    '/logs/bundle.zip::app.log',
    layout('automatic', 'archive-v1'),
    'stableAutomatic',
    2,
  );
  assert.equal(loadLogFieldLayout(storage, '/other/a.log'), null);
  assert.equal(loadLogFieldLayout(storage, '/logs/other.zip::app.log'), null);
  assert.equal(loadLogFieldLayout(storage, '/logs/bundle.zip::other.log'), null);
  assert.equal(loadLogFieldLayout(storage, '/logs/bundle.zip::app.log')?.fingerprint, 'archive-v1');
});

test('caps the store at 256 records and evicts the least recently used identity', () => {
  const storage = new MemoryStorage();
  for (let index = 0; index < 256; index += 1) {
    persistLogFieldLayout(
      storage,
      `/logs/${index}.log`,
      layout('automatic', `fingerprint-${index}`),
      'stableAutomatic',
      index + 1,
    );
  }
  assert.ok(loadLogFieldLayout(storage, '/logs/0.log', 1_000));
  persistLogFieldLayout(
    storage,
    '/logs/new.log',
    layout('automatic', 'new'),
    'stableAutomatic',
    1_001,
  );
  const snapshot = JSON.parse(storage.getItem(LOG_FIELD_LAYOUT_STORAGE_KEY)!) as {
    entries: { identity: string }[];
  };
  assert.equal(snapshot.entries.length, 256);
  assert.ok(loadLogFieldLayout(storage, '/logs/0.log', 1_002));
  assert.equal(loadLogFieldLayout(storage, '/logs/1.log', 1_003), null);
  assert.ok(loadLogFieldLayout(storage, '/logs/new.log', 1_004));
});

test('clears only the saved record and exposes fingerprint mismatch without changing storage', () => {
  const storage = new MemoryStorage();
  persistLogFieldLayout(storage, '/logs/a.log', layout(), 'stableAutomatic', 1);
  persistLogFieldLayout(storage, '/logs/b.log', layout('automatic', 'other'), 'stableAutomatic', 2);
  const restored = loadLogFieldLayout(storage, '/logs/a.log', 3)!;
  const writesBeforeCheck = storage.writes;
  assert.equal(savedLayoutFingerprintMatches(restored, 'bracketed-v1'), true);
  assert.equal(savedLayoutFingerprintMatches(restored, 'changed-layout'), false);
  assert.equal(storage.writes, writesBeforeCheck);
  assert.equal(clearSavedLogFieldLayout(storage, '/logs/a.log'), true);
  assert.equal(loadLogFieldLayout(storage, '/logs/a.log'), null);
  assert.ok(loadLogFieldLayout(storage, '/logs/b.log'));
});

test('storage failures are best effort and do not throw', () => {
  const storage = new MemoryStorage();
  persistLogFieldLayout(storage, '/logs/a.log', layout(), 'stableAutomatic', 1);
  storage.failWrites = true;
  assert.equal(
    persistLogFieldLayout(storage, '/logs/a.log', layout('manual', 'manual'), 'nameCommitted', 2),
    false,
  );
  assert.equal(clearSavedLogFieldLayout(storage, '/logs/a.log'), false);
  assert.deepEqual(loadLogFieldLayout(storage, '/logs/a.log', 3), layout());
  assert.equal(persistLogFieldLayout(storage, '', layout(), 'stableAutomatic', 4), false);
  assert.equal(
    persistLogFieldLayout(storage, '/logs/archive.zip::', layout(), 'stableAutomatic', 5),
    false,
  );
});
