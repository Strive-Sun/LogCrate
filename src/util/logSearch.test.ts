import assert from 'node:assert/strict';
import test from 'node:test';
import { findKeywordMatches } from './logSearch';

const defaults = { wholeWord: false, caseSensitive: false };

test('returns every non-overlapping keyword fragment', () => {
  assert.deepEqual(findKeywordMatches('error x errorerror', 'error', defaults), [
    { startColumn: 0, endColumn: 5 },
    { startColumn: 8, endColumn: 13 },
    { startColumn: 13, endColumn: 18 },
  ]);
});

test('whole-word matching rejects letters, numbers, and underscores at either boundary', () => {
  assert.deepEqual(
    findKeywordMatches('error errors myerror error_code error-2', 'error', {
      ...defaults,
      wholeWord: true,
    }),
    [
      { startColumn: 0, endColumn: 5 },
      { startColumn: 32, endColumn: 37 },
    ],
  );
});

test('case sensitivity and Unicode folding follow the navigation search semantics', () => {
  assert.deepEqual(findKeywordMatches('Äpfel äPFEL', 'äpfel', defaults), [
    { startColumn: 0, endColumn: 5 },
    { startColumn: 6, endColumn: 11 },
  ]);
  assert.deepEqual(
    findKeywordMatches('Error error ERROR', 'Error', { ...defaults, caseSensitive: true }),
    [{ startColumn: 0, endColumn: 5 }],
  );
  assert.deepEqual(findKeywordMatches('İ', 'i', defaults), []);
  assert.deepEqual(findKeywordMatches('İ', 'i\u0307', defaults), [
    { startColumn: 0, endColumn: 1 },
  ]);
});

test('reports JavaScript UTF-16 columns around supplementary characters', () => {
  assert.deepEqual(findKeywordMatches('A😀Error 😀error', 'error', defaults), [
    { startColumn: 3, endColumn: 8 },
    { startColumn: 11, endColumn: 16 },
  ]);
});

test('empty keywords never produce fragments', () => {
  assert.deepEqual(findKeywordMatches('error', '', defaults), []);
});
