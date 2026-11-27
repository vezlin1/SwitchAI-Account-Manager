import assert from 'node:assert/strict'
import { after, before, test } from 'node:test'
import { fileURLToPath } from 'node:url'
import { createElement } from 'react'
import { renderToStaticMarkup } from 'react-dom/server'
import { createServer } from 'vite'
import { accountAttention } from './accountFeedback.ts'

let server
let QuotaCell
let AccountStatus
let AccountDetails
before(async () => {
  server = await createServer({
    root: fileURLToPath(new URL('../../', import.meta.url)),
    configFile: false,
    cacheDir: 'node_modules/.vite-quota-tests',
    optimizeDeps: { noDiscovery: true },
    server: { middlewareMode: true, hmr: false, watch: null, ws: false }
  })
  QuotaCell = (await server.ssrLoadModule('/src/components/accounts/QuotaCell.tsx')).QuotaCell
  AccountStatus = (await server.ssrLoadModule('/src/components/accounts/AccountStatus.tsx')).AccountStatus
  AccountDetails = (await server.ssrLoadModule('/src/components/accounts/AccountDetails.tsx')).AccountDetails
})
after(() => server?.close())

const now = Math.floor(Date.now() / 1000)
const fetchedAt = now - 7200
function renderQuota(usedPercent, resetAt) {
  return renderToStaticMarkup(createElement(QuotaCell, {
    title: '5h', fetchedAt, window: { usedPercent, fetchedAt, resetAt, limitWindowSeconds: 18000 }
  }))
}

test('a saved percentage and bar remain visible an hour after the check', () => {
  const markup = renderQuota(28, now + 3600)
  assert.match(markup, /72% left/)
  assert.match(markup, /aria-valuenow="72"/)
  assert.match(markup, /Last checked:/)
  assert.match(markup, /Resets in /)
  assert.doesNotMatch(markup, /quota-fill-projected/)
})

test('a window whose reset has passed is projected as full until a check confirms it', () => {
  const markup = renderQuota(28, now - 3600)
  assert.match(markup, /~100% left/)
  assert.match(markup, /aria-valuenow="100"/)
  assert.match(markup, /quota-fill-projected/)
  assert.match(markup, /waiting for confirmation/)
  assert.match(markup, /Reset · not confirmed yet/)
  assert.doesNotMatch(markup, /Resets in /)
})

test('zero and full saved balances are distinct from absent or invalid readings', () => {
  assert.match(renderQuota(100, null), /Used up/)
  assert.match(renderQuota(0, null), /100% left/)
  for (const value of [null, NaN, Infinity, -1, 101]) {
    const markup = renderQuota(value, null)
    assert.match(markup, /No data yet/)
    assert.doesNotMatch(markup, /progressbar/)
  }
})

const savedWindow = { usedPercent: 28, fetchedAt, resetAt: now - 60, limitWindowSeconds: 18000 }
const savedQuota = { primary: savedWindow, secondary: savedWindow, fetchedAt }

function renderStatuses(tokenStatus, { quota = savedQuota, isRefreshing = false, issues = {} } = {}) {
  const account = { id: 'test-account', provider: 'gemini', tokenHealth: { status: tokenStatus }, quota, issues }
  return [
    renderToStaticMarkup(createElement(AccountStatus, { account, attention: accountAttention(account), isRefreshing })),
    renderToStaticMarkup(createElement(AccountDetails, {
      account, recommendation: null, busyKeys: new Set(isRefreshing ? ['quota:test-account'] : [])
    }))
  ]
}

test('a healthy account says nothing in the list and stays healthy in details', () => {
  for (const tokenStatus of ['healthy', 'refreshed']) {
    for (const quota of [savedQuota, null]) {
      const [listView, detailsView] = renderStatuses(tokenStatus, { quota })
      assert.equal(listView, '')
      assert.match(detailsView, />Healthy</)
      assert.match(detailsView, /last recorded connection check succeeded/)
      assert.doesNotMatch(detailsView, /Refresh quota|Saved quota/)
    }
  }
})

test('a used-up window says when it is back instead of 0% left', () => {
  const markup = renderQuota(100, now + 40 * 60)
  assert.match(markup, /Used up/)
  assert.match(markup, /Back in 3\dm|Back in 40m/)
  assert.doesNotMatch(markup, /0% left/)
})

test('a weekly reset counts down in days', () => {
  const markup = renderQuota(10, now + 3 * 86400 + 5 * 3600 + 120)
  assert.match(markup, /Resets in 3d 5h/)
})

test('unknown, pending and failed checks cannot be mistaken for healthy in either view', () => {
  for (const [tokenStatus, options, label] of [
    ['unknown', {}, 'Not checked'],
    ['healthy', { isRefreshing: true }, 'Checking'],
    ['healthy', { issues: { quota: 'timeout' } }, 'Couldn’t check limits'],
    ['healthy', { issues: { subscription: 'timeout' } }, 'Couldn’t check the plan'],
    ['needs_relogin', {}, 'Signed out'],
    ['network_error', {}, 'Couldn’t check limits'],
    ['server_error', {}, 'Couldn’t check limits']
  ]) {
    for (const view of renderStatuses(tokenStatus, options)) {
      assert.ok(view.includes(label), label)
      assert.doesNotMatch(view, />Healthy</)
    }
  }
  // Missing quota is already said by the quota cells, so the list stays quiet.
  const [listView, detailsView] = renderStatuses('unknown', { quota: null })
  assert.equal(listView, '')
  assert.ok(detailsView.includes('No quota yet'))
})
