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
  assert.match(markup, /Reset in /)
  assert.doesNotMatch(markup, /quota-fill-projected/)
})

test('a window whose reset has passed is projected as full until a check confirms it', () => {
  const markup = renderQuota(28, now - 3600)
  assert.match(markup, /~100% left/)
  assert.match(markup, /aria-valuenow="100"/)
  assert.match(markup, /quota-fill-projected/)
  assert.match(markup, /waiting for confirmation/)
  assert.match(markup, /Reset · not confirmed yet/)
  assert.doesNotMatch(markup, /Reset in /)
})

test('zero and full saved balances are distinct from absent or invalid readings', () => {
  assert.match(renderQuota(100, null), /0% left/)
  assert.match(renderQuota(0, null), /100% left/)
  for (const value of [null, NaN, Infinity, -1, 101]) {
    const markup = renderQuota(value, null)
    assert.match(markup, /Not available/)
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

test('saved local health survives quota age, reset and absence in both account views', () => {
  for (const tokenStatus of ['healthy', 'refreshed']) {
    for (const quota of [savedQuota, null]) {
      for (const view of renderStatuses(tokenStatus, { quota })) {
        assert.match(view, />Healthy</)
        assert.match(view, /last recorded connection check succeeded/)
        assert.doesNotMatch(view, /Refresh quota|Saved quota/)
      }
    }
  }
})

test('unknown, pending and failed checks cannot be mistaken for healthy in either view', () => {
  for (const [tokenStatus, options, label] of [
    ['unknown', {}, 'Not checked'],
    ['unknown', { quota: null }, 'No quota yet'],
    ['healthy', { isRefreshing: true }, 'Checking'],
    ['healthy', { issues: { quota: 'timeout' } }, 'Quota refresh failed'],
    ['healthy', { issues: { subscription: 'timeout' } }, 'Plan refresh failed'],
    ['needs_relogin', {}, 'Sign-in required'],
    ['network_error', {}, 'Quota refresh failed'],
    ['server_error', {}, 'Quota refresh failed']
  ]) {
    for (const view of renderStatuses(tokenStatus, options)) {
      assert.ok(view.includes(label), label)
      assert.doesNotMatch(view, />Healthy</)
    }
  }
})
