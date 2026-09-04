import assert from 'node:assert/strict'
import test from 'node:test'
import { settingsPatch } from './settingsPatch.ts'

test('update checks can be disabled and enabled through the settings save queue', () => {
  for (const enabled of [false, true]) {
    const authoritative = { autoCheckUpdates: !enabled, closeToTray: true }
    const draft = { ...authoritative, autoCheckUpdates: enabled }
    const requested = { ...authoritative, ...settingsPatch(authoritative, draft) }
    assert.equal(requested.autoCheckUpdates, enabled)
    assert.equal(requested.closeToTray, true)
  }
})

test('unrelated settings edits preserve an update version dismissed on the server', () => {
  const previous = { autoCheckUpdates: true, ignoredUpdateVersion: null, closeToTray: true }
  const draft = { ...previous, closeToTray: false }
  const authoritative = { ...previous, ignoredUpdateVersion: '2.0.0' }
  const requested = { ...authoritative, ...settingsPatch(previous, draft) }
  assert.equal(requested.ignoredUpdateVersion, '2.0.0')
  assert.equal(requested.closeToTray, false)
})

test('privacy mode changes are captured in settings patch and merged cleanly', () => {
  for (const enabled of [false, true]) {
    const previous = { privacyMode: !enabled, closeToTray: true }
    const draft = { ...previous, privacyMode: enabled }
    const patch = settingsPatch(previous, draft)
    assert.equal(patch.privacyMode, enabled)
    const merged = { ...previous, ...patch }
    assert.equal(merged.privacyMode, enabled)
    assert.equal(merged.closeToTray, true)
  }
})

