import assert from 'node:assert/strict';
import test from 'node:test';
import { resizeMainWorkspaceFromWindowEdge } from './aiWorkspaceResize';

test('left-edge resize changes only the main workspace width', () => {
  assert.equal(
    resizeMainWorkspaceFromWindowEdge(
      { screenX: 100, innerWidth: 1440 },
      { screenX: 340, innerWidth: 1200 },
      1000,
    ),
    760,
  );
  assert.equal(
    resizeMainWorkspaceFromWindowEdge(
      { screenX: 340, innerWidth: 1200 },
      { screenX: 140, innerWidth: 1400 },
      760,
    ),
    960,
  );
});

test('right-edge resize keeps the main workspace fixed so only AI width changes', () => {
  assert.equal(
    resizeMainWorkspaceFromWindowEdge(
      { screenX: 100, innerWidth: 1440 },
      { screenX: 100, innerWidth: 1200 },
      1000,
    ),
    1000,
  );
});

test('moving the whole window does not change either workspace width', () => {
  assert.equal(
    resizeMainWorkspaceFromWindowEdge(
      { screenX: 100, innerWidth: 1440 },
      { screenX: 300, innerWidth: 1440 },
      1000,
    ),
    1000,
  );
});

test('small frame-coordinate rounding still identifies a left-edge resize', () => {
  assert.equal(
    resizeMainWorkspaceFromWindowEdge(
      { screenX: 100, innerWidth: 1440 },
      { screenX: 299, innerWidth: 1240 },
      1000,
    ),
    800,
  );
});
