import fs from 'node:fs'
import path from 'node:path'
import crypto from 'node:crypto'
import { fileURLToPath } from 'node:url'

const __filename = fileURLToPath(import.meta.url)
const __dirname = path.dirname(__filename)
const repoRoot = path.resolve(__dirname, '..')

// Read package.json for default version
const packageJsonPath = path.join(repoRoot, 'package.json')
const packageJson = JSON.parse(fs.readFileSync(packageJsonPath, 'utf8'))
const rawVersion = process.env.RELEASE_VERSION || packageJson.version
const version = String(rawVersion).trim().replace(/^v/, '')

// Paths
const distDir = path.join(repoRoot, 'dist', 'release')
const exePath = path.join(distDir, 'SwitchAI.exe')
const sigPath = path.join(distDir, 'SwitchAI.exe.sig')
const outputPath = path.join(distDir, 'latest.json')
const releaseNotesPath = path.join(repoRoot, 'RELEASE_NOTES.md')

// Read release notes if present
let notes = `SwitchAI Account Manager v${version}`
if (fs.existsSync(releaseNotesPath)) {
  const content = fs.readFileSync(releaseNotesPath, 'utf8').trim()
  if (content.length > 0) {
    notes = content
  }
}

// Calculate SHA-256 and size of executable if it exists
let sha256 = ''
let size = 0
if (fs.existsSync(exePath)) {
  const fileBuffer = fs.readFileSync(exePath)
  size = fileBuffer.length
  sha256 = crypto.createHash('sha256').update(fileBuffer).digest('hex')
} else {
  console.warn(`[generate-update-manifest] Executable not found at: ${exePath}`)
}

// Read signature
if (!fs.existsSync(sigPath)) {
  console.error(`[generate-update-manifest] Error: Signature file not found at: ${sigPath}`)
  console.error('[generate-update-manifest] An update manifest cannot be generated without a valid cryptographic signature.')
  process.exit(1)
}

const signature = fs.readFileSync(sigPath, 'utf8').trim()
if (!signature) {
  console.error(`[generate-update-manifest] Error: Signature file is empty at: ${sigPath}`)
  console.error('[generate-update-manifest] An update manifest cannot be generated with an empty signature.')
  process.exit(1)
}

const manifest = {
  version,
  notes,
  pub_date: new Date().toISOString(),
  platforms: {
    'windows-x86_64': {
      signature,
      url: `https://github.com/vezlin1/SwitchAI-Account-Manager/releases/download/v${version}/SwitchAI.exe`,
      sha256: sha256 || undefined,
      size: size || undefined
    }
  }
}

// Ensure dist directory exists
if (!fs.existsSync(distDir)) {
  fs.mkdirSync(distDir, { recursive: true })
}

fs.writeFileSync(outputPath, JSON.stringify(manifest, null, 2), 'utf8')
console.log(`[generate-update-manifest] Successfully generated update manifest at: ${outputPath}`)
console.log(JSON.stringify(manifest, null, 2))
