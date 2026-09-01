import fs from 'node:fs'
import path from 'node:path'
import process from 'node:process'

const rootPackage = JSON.parse(fs.readFileSync('package.json', 'utf8'))
const uiPackage = JSON.parse(fs.readFileSync('ui/package.json', 'utf8'))
const tauriConfig = JSON.parse(fs.readFileSync('src-tauri/tauri.conf.json', 'utf8'))
const cargoToml = fs.readFileSync('src-tauri/Cargo.toml', 'utf8')
const cargoVersion = cargoToml.match(/^version\s*=\s*"([^"]+)"/m)?.[1]

const versions = new Map([
  ['package.json', rootPackage.version],
  ['ui/package.json', uiPackage.version],
  ['src-tauri/tauri.conf.json', tauriConfig.version],
  ['src-tauri/Cargo.toml', cargoVersion]
])
const expected = rootPackage.version
const mismatches = [...versions].filter(([, version]) => version !== expected)
if (mismatches.length) {
  throw new Error(`Version mismatch: ${[...versions].map(([file, version]) => `${file}=${version}`).join(', ')}`)
}

const refName = process.env.GITHUB_REF_NAME
if (refName?.startsWith('v') && refName !== `v${expected}`) {
  throw new Error(`Release tag ${refName} does not match application version v${expected}`)
}

const failures = []
const escapeRegExp = (value) => value.replace(/[.*+?^${}()|[\]\\]/g, '\\$&')
const versionLiteral = `['"]${escapeRegExp(expected)}['"]`
const hardcodedFallbackPatterns = [
  new RegExp(`useState\\s*\\(\\s*${versionLiteral}`),
  new RegExp(`(?:setAppVersion|appVersion)\\b[^\\n\\r]*${versionLiteral}`)
]
const liteWord = /\bLite\b/
const legacyMigrationReference = /CodexAccountManagerLite|legacy/i

function walkFiles(directory) {
  const results = []
  for (const entry of fs.readdirSync(directory, { withFileTypes: true })) {
    const fullPath = path.join(directory, entry.name)
    if (entry.isDirectory()) {
      results.push(...walkFiles(fullPath))
    }
    else if (entry.isFile()) {
      results.push(fullPath)
    }
  }
  return results
}

function checkFile(file, sourceRoot) {
  const relativePath = path.relative(sourceRoot, file).replaceAll(path.sep, '/')
  if (relativePath === 'assets' || relativePath.startsWith('assets/')) {
    return
  }

  const lines = fs.readFileSync(file, 'utf8').split(/\r?\n/)
  for (let index = 0; index < lines.length; index += 1) {
    const line = lines[index]
    const location = `${path.relative(process.cwd(), file).replaceAll(path.sep, '/')}:${index + 1}`
    if (hardcodedFallbackPatterns.some((pattern) => pattern.test(line))) {
      failures.push(`${location}: hardcoded UI version fallback '${expected}' is not allowed; read the version from getVersion()`)
    }
    if (liteWord.test(line) && !legacyMigrationReference.test(line)) {
      failures.push(`${location}: user-facing "Lite" is not allowed except for legacy migration references`)
    }
  }
}

const uiSourceRoot = path.join(process.cwd(), 'ui', 'src')
if (fs.existsSync(uiSourceRoot)) {
  for (const file of walkFiles(uiSourceRoot)) {
    checkFile(file, uiSourceRoot)
  }
}

const readme = path.join(process.cwd(), 'README.md')
if (fs.existsSync(readme)) {
  checkFile(readme, path.dirname(readme))
}

if (failures.length) {
  throw new Error(`Version/release verification failed:\n${failures.join('\n')}`)
}

console.log(`Version consistency verified: ${expected}`)
