import assert from 'node:assert/strict'
import test from 'node:test'
import { formatEventTime } from './eventTime.ts'

const now = Date.UTC(2026, 8, 8, 12)

test('event times use English relative labels and exact timestamps', () => {
  const value = formatEventTime(now / 1000 - 300, now)
  assert.equal(value.relative, '5 minutes ago')
  assert.equal(value.iso, '2026-09-08T11:55:00.000Z')
  assert.match(value.exact, /Sep/)
  assert.match(value.exact, /2026/)
  assert.equal(formatEventTime(now / 1000 - 10, now).relative, 'Just now')
  assert.equal(formatEventTime(now / 1000 - 7200, now).relative, '2 hours ago')
  assert.equal(formatEventTime(now / 1000 - 86400, now).relative, '1 day ago')
  assert.equal(formatEventTime(now / 1000 + 300, now).relative, 'in 5 minutes')
})

test('missing or invalid event times have no relative or exact date', () => {
  for (const value of [null, undefined, NaN, Infinity, 1e20]) {
    assert.equal(formatEventTime(value, now), null)
  }
})
