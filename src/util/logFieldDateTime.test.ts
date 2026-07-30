import assert from 'node:assert/strict';
import test from 'node:test';
import {
  displayLogMinute,
  formatPickerMinute,
  parseLogMinute,
  toPickerMinute,
} from './logFieldDateTime';

test('converts supported log timestamps to minute-only picker values', () => {
  assert.equal(toPickerMinute('2026-07-28 17:16:44.251'), '2026-07-28T17:16');
  assert.equal(toPickerMinute('2026/07/28T17:16:44Z'), '2026-07-28T17:16');
  assert.equal(toPickerMinute('07-28 17:16:44.251', 2026), '2026-07-28T17:16');
  assert.equal(toPickerMinute('0728/171644', 2026), '2026-07-28T17:16');
});

test('formats picker minutes in the current log field format without seconds', () => {
  assert.equal(
    formatPickerMinute('2026-07-29T08:05', '2026-07-28 17:16:44.251'),
    '2026-07-29 08:05',
  );
  assert.equal(formatPickerMinute('2026-07-29T08:05', '07-28 17:16:44.251'), '07-29 08:05');
  assert.equal(formatPickerMinute('2026-07-29T08:05', '0728/171644'), '0729/0805');
  assert.equal(displayLogMinute('2026-07-28 17:16:44.251'), '2026-07-28 17:16');
});

test('rejects invalid dates and times', () => {
  assert.equal(parseLogMinute('2026-02-30 12:00'), null);
  assert.equal(parseLogMinute('2026-07-28 24:00'), null);
  assert.equal(toPickerMinute('not a timestamp'), '');
  assert.equal(formatPickerMinute('invalid', '2026-07-28 17:16:44.251'), undefined);
});
