import assert from 'node:assert/strict'
import test from 'node:test'
import { settingsPatch } from './settingsPatch.ts'

test('quota refresh can be disabled and enabled through the settings save queue', () => {
  for (const enabled of [false, true]) {
    const authoritative = { autoRefreshEnabled: !enabled, closeToTray: true }
    const draft = { ...authoritative, autoRefreshEnabled: enabled }
    const requested = { ...authoritative, ...settingsPatch(authoritative, draft) }
    assert.equal(requested.autoRefreshEnabled, enabled)
    assert.equal(requested.closeToTray, true)
  }
})

test('unrelated settings edits preserve a server-side privacy setting change', () => {
  const previous = { privacyMode: false, closeToTray: true }
  const draft = { ...previous, closeToTray: false }
  const authoritative = { ...previous, privacyMode: true }
  const requested = { ...authoritative, ...settingsPatch(previous, draft) }
  assert.equal(requested.privacyMode, true)
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

test('settings arrays are compared by ordered values', () => {
  const previous = { hiddenAccountIds: ['a'], enabledProviders: ['codex', 'gemini'] }
  const unchanged = { hiddenAccountIds: ['a'], enabledProviders: ['codex', 'gemini'] }
  assert.deepEqual(settingsPatch(previous, unchanged), {})

  const changed = { hiddenAccountIds: ['a'], enabledProviders: ['gemini', 'codex'] }
  assert.deepEqual(settingsPatch(previous, changed), { enabledProviders: ['gemini', 'codex'] })
  assert.deepEqual(settingsPatch({}, { geminiSwitchTargets: [] }), { geminiSwitchTargets: [] })
})

test('an unrelated settings save preserves tag memberships and preferences', () => {
  const previous = { accountTags: [{ id: 'tag:a', accountIds: ['one'], showInAll: true }], closeToTray: false }
  const draft = structuredClone(previous)
  draft.closeToTray = true
  assert.deepEqual(settingsPatch(previous, draft), { closeToTray: true })
  draft.accountTags[0].showInAll = false
  assert.deepEqual(settingsPatch(previous, draft).accountTags, draft.accountTags)
})

