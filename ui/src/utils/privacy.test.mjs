import test from 'node:test'
import assert from 'node:assert/strict'

function maskEmail(email) {
  if (!email) return '••••••'
  const atIndex = email.indexOf('@')
  if (atIndex <= 0) return '••••••••'
  const local = email.slice(0, atIndex)
  const domain = email.slice(atIndex)

  if (local.length <= 2) {
    return `${local[0]}•••${domain}`
  }
  if (local.length <= 4) {
    return `${local[0]}••••${local.slice(-1)}${domain}`
  }
  return `${local.slice(0, 2)}••••••${local.slice(-1)}${domain}`
}

function maskAccountId(id) {
  if (!id) return '••••••••'
  if (id.length <= 6) return '••••••••'
  return `${id.slice(0, 3)}••••${id.slice(-3)}`
}

test('maskEmail properly obscures emails of various lengths', () => {
  assert.equal(maskEmail('a@gmail.com'), 'a•••@gmail.com')
  assert.equal(maskEmail('ab@gmail.com'), 'a•••@gmail.com')
  assert.equal(maskEmail('user@gmail.com'), 'u••••r@gmail.com')
  assert.equal(maskEmail('my_account@gmail.com'), 'my••••••t@gmail.com')
  assert.equal(maskEmail(''), '••••••')
  assert.equal(maskEmail(null), '••••••')
})

test('maskAccountId obscures IDs correctly', () => {
  assert.equal(maskAccountId('acc_12345678'), 'acc••••678')
  assert.equal(maskAccountId('short'), '••••••••')
  assert.equal(maskAccountId(null), '••••••••')
})
