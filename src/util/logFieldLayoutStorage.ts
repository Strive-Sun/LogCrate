export const LOG_FIELD_LAYOUT_STORAGE_KEY = 'logcrate.logFieldLayouts.v1';

const STORAGE_VERSION = 1;
const MAX_SAVED_LAYOUTS = 256;
const MAX_IDENTITY_LENGTH = 8192;
const MAX_FIELD_TEXT_LENGTH = 256;
const MAX_FINGERPRINT_LENGTH = 4096;
const MAX_BOUNDARY = 1_000_000;

interface StorageLike {
  getItem(key: string): string | null;
  setItem(key: string, value: string): void;
}

export type LogFieldType = 'time' | 'level' | 'discrete' | 'text';
export type LogFieldLayoutSource = 'automatic' | 'manual';

export interface StoredLogFieldDefinition {
  id: string;
  name: string;
  type: LogFieldType;
  start: number;
  end: number | null;
}

export interface StoredLogFieldLayout {
  fields: StoredLogFieldDefinition[];
  fingerprint: string;
  encodingHint?: string;
  source: LogFieldLayoutSource;
}

export type LayoutPersistenceTrigger =
  | 'stableAutomatic'
  | 'boundaryDragCommitted'
  | 'nameCommitted'
  | 'typeChanged'
  | 'fieldSplit'
  | 'fieldMerged'
  | 'boundaryDragging'
  | 'nameEditing'
  | 'invalid';

interface StoredLayoutRecord {
  identity: string;
  layout: StoredLogFieldLayout;
  updatedAt: number;
  lastUsedAt: number;
}

interface LayoutStore {
  version: number;
  entries: StoredLayoutRecord[];
}

const COMMIT_TRIGGERS = new Set<LayoutPersistenceTrigger>([
  'stableAutomatic',
  'boundaryDragCommitted',
  'nameCommitted',
  'typeChanged',
  'fieldSplit',
  'fieldMerged',
]);

function normalizeSegments(value: string, absolute: boolean): string {
  const parts: string[] = [];
  for (const part of value.split('/')) {
    if (!part || part === '.') continue;
    if (part === '..') {
      if (parts.length > 0 && parts[parts.length - 1] !== '..') parts.pop();
      else if (!absolute) parts.push(part);
      continue;
    }
    parts.push(part);
  }
  return parts.join('/');
}

function normalizeOuterPath(path: string): string {
  const slashed = path.trim().replace(/\\/g, '/');
  const windowsDrive = /^([a-zA-Z]):(?:\/|$)/.exec(slashed);
  const unc = slashed.startsWith('//');
  if (windowsDrive) {
    const body = normalizeSegments(slashed.slice(2), true);
    return `${windowsDrive[1]}:/${body}`.replace(/\/$/, '').toLowerCase();
  }
  if (unc) {
    return `//${normalizeSegments(slashed.slice(2), true)}`.toLowerCase();
  }
  if (slashed.startsWith('/')) return `/${normalizeSegments(slashed.slice(1), true)}`;
  return normalizeSegments(slashed, false);
}

function normalizeArchiveEntry(path: string): string {
  return normalizeSegments(path.trim().replace(/\\/g, '/').replace(/^\/+/, ''), false);
}

export function normalizeLogFileIdentity(entryKey: string): string {
  const separator = entryKey.indexOf('::');
  if (separator < 0) return `file:${normalizeOuterPath(entryKey)}`;
  const outer = normalizeOuterPath(entryKey.slice(0, separator));
  const entry = normalizeArchiveEntry(entryKey.slice(separator + 2));
  return `archive:${outer}::${entry}`;
}

function isFiniteTimestamp(value: unknown): value is number {
  return typeof value === 'number' && Number.isFinite(value) && value >= 0;
}

function validText(value: unknown, maxLength: number): value is string {
  return (
    typeof value === 'string' &&
    value.length > 0 &&
    value.length <= maxLength &&
    !/[\u0000-\u001f\u007f]/.test(value)
  );
}

function validIdentity(identity: string): boolean {
  if (!validText(identity, MAX_IDENTITY_LENGTH)) return false;
  if (identity.startsWith('file:')) return identity.length > 'file:'.length;
  if (!identity.startsWith('archive:')) return false;
  const separator = identity.indexOf('::', 'archive:'.length);
  return separator > 'archive:'.length && separator + 2 < identity.length;
}

function validLayout(value: unknown): value is StoredLogFieldLayout {
  if (!value || typeof value !== 'object') return false;
  const layout = value as Partial<StoredLogFieldLayout>;
  if (
    !Array.isArray(layout.fields) ||
    layout.fields.length === 0 ||
    layout.fields.length > 128 ||
    !validText(layout.fingerprint, MAX_FINGERPRINT_LENGTH) ||
    (layout.encodingHint !== undefined && !validText(layout.encodingHint, MAX_FIELD_TEXT_LENGTH)) ||
    (layout.source !== 'automatic' && layout.source !== 'manual')
  ) {
    return false;
  }

  const ids = new Set<string>();
  let previousEnd = 0;
  for (let index = 0; index < layout.fields.length; index += 1) {
    const field = layout.fields[index] as Partial<StoredLogFieldDefinition>;
    if (
      !validText(field.id, MAX_FIELD_TEXT_LENGTH) ||
      ids.has(field.id) ||
      !validText(field.name, MAX_FIELD_TEXT_LENGTH) ||
      !['time', 'level', 'discrete', 'text'].includes(field.type ?? '') ||
      !Number.isInteger(field.start) ||
      field.start! < 0 ||
      field.start! > MAX_BOUNDARY ||
      (field.end !== null &&
        (!Number.isInteger(field.end) ||
          field.end! <= field.start! ||
          field.end! > MAX_BOUNDARY)) ||
      field.start! < previousEnd ||
      (field.end === null && index !== layout.fields.length - 1)
    ) {
      return false;
    }
    ids.add(field.id);
    previousEnd = field.end ?? field.start!;
  }
  return true;
}

function cloneLayout(layout: StoredLogFieldLayout): StoredLogFieldLayout {
  return { ...layout, fields: layout.fields.map((field) => ({ ...field })) };
}

function validRecord(value: unknown): value is StoredLayoutRecord {
  if (!value || typeof value !== 'object') return false;
  const record = value as Partial<StoredLayoutRecord>;
  return (
    typeof record.identity === 'string' &&
    validIdentity(record.identity) &&
    validLayout(record.layout) &&
    isFiniteTimestamp(record.updatedAt) &&
    isFiniteTimestamp(record.lastUsedAt)
  );
}

function readStore(storage: StorageLike): LayoutStore | null {
  try {
    const raw = storage.getItem(LOG_FIELD_LAYOUT_STORAGE_KEY);
    if (!raw) return { version: STORAGE_VERSION, entries: [] };
    const parsed = JSON.parse(raw) as Partial<LayoutStore>;
    if (parsed.version !== STORAGE_VERSION || !Array.isArray(parsed.entries)) return null;
    const identities = new Set<string>();
    const entries: StoredLayoutRecord[] = [];
    for (const value of parsed.entries) {
      if (!validRecord(value) || identities.has(value.identity)) continue;
      identities.add(value.identity);
      entries.push({ ...value, layout: cloneLayout(value.layout) });
    }
    return { version: STORAGE_VERSION, entries };
  } catch {
    return null;
  }
}

function writeStore(storage: StorageLike, entries: StoredLayoutRecord[]): boolean {
  try {
    storage.setItem(
      LOG_FIELD_LAYOUT_STORAGE_KEY,
      JSON.stringify({ version: STORAGE_VERSION, entries } satisfies LayoutStore),
    );
    return true;
  } catch {
    return false;
  }
}

function isCommittedTrigger(trigger: LayoutPersistenceTrigger): boolean {
  return COMMIT_TRIGGERS.has(trigger);
}

export function persistLogFieldLayout(
  storage: StorageLike,
  entryKey: string,
  layout: StoredLogFieldLayout,
  trigger: LayoutPersistenceTrigger,
  now = Date.now(),
): boolean {
  if (!isCommittedTrigger(trigger) || !validLayout(layout) || !isFiniteTimestamp(now)) return false;
  if (trigger === 'stableAutomatic' ? layout.source !== 'automatic' : layout.source !== 'manual') {
    return false;
  }

  const identity = normalizeLogFileIdentity(entryKey);
  if (!validIdentity(identity)) return false;
  const store = readStore(storage) ?? { version: STORAGE_VERSION, entries: [] };
  const previous = store.entries.find((entry) => entry.identity === identity);
  const record: StoredLayoutRecord = {
    identity,
    layout: cloneLayout(layout),
    updatedAt: now,
    lastUsedAt: now,
  };
  const entries = store.entries.filter((entry) => entry.identity !== identity);
  entries.push(previous ? { ...record, updatedAt: now } : record);
  entries.sort((left, right) => right.lastUsedAt - left.lastUsedAt);
  return writeStore(storage, entries.slice(0, MAX_SAVED_LAYOUTS));
}

export function loadLogFieldLayout(
  storage: StorageLike,
  entryKey: string,
  now = Date.now(),
): StoredLogFieldLayout | null {
  const store = readStore(storage);
  if (!store) return null;
  const identity = normalizeLogFileIdentity(entryKey);
  const record = store.entries.find((entry) => entry.identity === identity);
  if (!record) return null;
  if (isFiniteTimestamp(now)) {
    record.lastUsedAt = now;
    store.entries.sort((left, right) => right.lastUsedAt - left.lastUsedAt);
    writeStore(storage, store.entries.slice(0, MAX_SAVED_LAYOUTS));
  }
  return cloneLayout(record.layout);
}

export function clearSavedLogFieldLayout(storage: StorageLike, entryKey: string): boolean {
  const store = readStore(storage);
  if (!store) return false;
  const identity = normalizeLogFileIdentity(entryKey);
  return writeStore(
    storage,
    store.entries.filter((entry) => entry.identity !== identity),
  );
}

export function savedLayoutFingerprintMatches(
  layout: StoredLogFieldLayout,
  currentFingerprint: string,
): boolean {
  return validLayout(layout) && layout.fingerprint === currentFingerprint;
}
