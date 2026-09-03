export function maskEmail(email?: string | null): string {
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

export function maskAccountId(id?: string | null): string {
  if (!id) return '••••••••'
  if (id.length <= 6) return '••••••••'
  return `${id.slice(0, 3)}••••${id.slice(-3)}`
}
