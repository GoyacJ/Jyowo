import { existsSync, readFileSync, readdirSync, statSync } from 'node:fs'
import { basename, dirname, extname, join, relative, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'

const repoRoot = resolve(dirname(fileURLToPath(import.meta.url)), '..')

function repoRel(p) {
  return relative(repoRoot, p)
}

function readLines(p) {
  try {
    return readFileSync(p, 'utf8').split(/\r?\n/)
  } catch {
    return []
  }
}

function lineCount(p) {
  try {
    return readFileSync(p, 'utf8').split(/\r?\n/).length
  } catch {
    return 0
  }
}

function walkFiles(dir, exts, maxDepth = 20) {
  const results = []
  function walk(d, depth) {
    if (depth > maxDepth) return
    let entries
    try {
      entries = readdirSync(d, { withFileTypes: true })
    } catch {
      return
    }
    for (const e of entries) {
      const fp = join(d, e.name)
      if (e.isDirectory()) {
        if (e.name === 'node_modules' || e.name === 'target' || e.name === '.git') continue
        walk(fp, depth + 1)
      } else if (e.isFile() && exts.some((ext) => fp.endsWith(ext))) {
        results.push(fp)
      }
    }
  }
  walk(dir, 0)
  return results.sort()
}

function walkAllFiles(dir, maxDepth = 20) {
  const results = []
  function walk(d, depth) {
    if (depth > maxDepth) return
    let entries
    try {
      entries = readdirSync(d, { withFileTypes: true })
    } catch {
      return
    }
    for (const e of entries) {
      const fp = join(d, e.name)
      if (e.isDirectory()) {
        if (e.name === 'node_modules' || e.name === 'target' || e.name === '.git') continue
        walk(fp, depth + 1)
      } else if (e.isFile()) {
        results.push(fp)
      }
    }
  }
  walk(dir, 0)
  return results.sort()
}

function countRustTestFunctions(filePath) {
  const lines = readLines(filePath)
  let count = 0
  for (const line of lines) {
    if (/^\s*#\[(tokio::)?test\]/.test(line) || /^\s*#\[test\]/.test(line)) {
      count++
    }
  }
  return count
}

// ── Scan ──

const frontendTestFiles = [
  ...walkFiles(join(repoRoot, 'apps/desktop/src'), ['.test.ts', '.test.tsx']),
]
const storybookFiles = [
  ...walkFiles(join(repoRoot, 'apps/desktop/src'), ['.stories.tsx', '.stories.ts']),
]
const playwrightFiles = [
  ...walkFiles(join(repoRoot, 'apps/desktop/e2e'), ['.spec.ts']),
]
const tauriTestFiles = [
  ...walkFiles(join(repoRoot, 'apps/desktop/src-tauri/tests'), ['.rs']),
]
const crateTestFiles = []
const crateDirs = readdirSync(join(repoRoot, 'crates'), { withFileTypes: true })
  .filter((e) => e.isDirectory())
  .map((e) => join(repoRoot, 'crates', e.name))
for (const crateDir of crateDirs) {
  const testsDir = join(crateDir, 'tests')
  if (existsSync(testsDir)) {
    crateTestFiles.push(...walkFiles(testsDir, ['.rs']))
  }
}
const allRustTestFiles = [...tauriTestFiles, ...crateTestFiles]

const scriptTestFiles = [
  ...walkFiles(join(repoRoot, 'scripts'), ['.test.mjs', '.test.ts']),
]

// ── Counts ──

let frontendTestCases = 0
for (const f of frontendTestFiles) {
  const lines = readLines(f)
  for (const line of lines) {
    if (/(?:it|test)\s*\(/.test(line) && !/^\s*\/\//.test(line)) {
      frontendTestCases++
    }
  }
}

let rustTestFnCount = 0
for (const f of allRustTestFiles) {
  rustTestFnCount += countRustTestFunctions(f)
}

// ── Line counts and oversized detection ──

const allTestFiles = [
  ...frontendTestFiles.map((p) => ({ path: p, kind: 'frontend' })),
  ...allRustTestFiles.map((p) => ({ path: p, kind: 'rust' })),
  ...storybookFiles.map((p) => ({ path: p, kind: 'storybook' })),
  ...playwrightFiles.map((p) => ({ path: p, kind: 'playwright' })),
  ...scriptTestFiles.map((p) => ({ path: p, kind: 'script' })),
]

const fileLineCounts = allTestFiles.map((f) => ({
  ...f,
  lines: lineCount(f.path),
}))

fileLineCounts.sort((a, b) => b.lines - a.lines)

const over1200 = fileLineCounts.filter((f) => f.lines > 1200)
const over800 = fileLineCounts.filter((f) => f.lines > 800 && f.lines <= 1200)

// ── Disallowed names ──

const disallowedNames = allTestFiles.filter((f) => {
  const name = basename(f.path)
  return (
    name.startsWith('spike_') ||
    /^m\d+_/.test(name) ||
    /^t\d+_/.test(name) ||
    (/_e2e\.rs$/.test(name) && !repoRel(f.path).startsWith('apps/desktop/src-tauri/tests/'))
  )
})

// ── Ignored/manual/stress tests ──

const ignoredTests = []
const manualLiveTests = []
const stressTests = []
for (const fp of allRustTestFiles) {
  const name = basename(fp)
  const content = readFileSync(fp, 'utf8')
  const lines = content.split(/\r?\n/)
  const hasIgnoreAttr = lines.some((l) => /^\s*#\[ignore\]/.test(l.trim()))
  if (hasIgnoreAttr) {
    ignoredTests.push({ path: fp, name })
  }
  if (name.startsWith('manual_live_')) {
    manualLiveTests.push({ path: fp, name, ignored: hasIgnoreAttr })
  }
  if (name.startsWith('stress_')) {
    stressTests.push({ path: fp, name, ignored: hasIgnoreAttr })
  }
}

// ── createTestCommandClient usage ──

const cmdClientUsage = new Map()
const srcDir = join(repoRoot, 'apps', 'desktop', 'src')
for (const f of walkAllFiles(srcDir)) {
  const ext = extname(f)
  if (!['.ts', '.tsx'].includes(ext)) continue
  const content = readFileSync(f, 'utf8')
  if (content.includes('createTestCommandClient')) {
    const count = (content.match(/createTestCommandClient/g) || []).length
    cmdClientUsage.set(repoRel(f), count)
  }
}
const cmdClientUsageSorted = [...cmdClientUsage.entries()].sort((a, b) => b[1] - a[1])

// ── Storybook by feature ──

const storybookByFeature = new Map()
for (const f of storybookFiles) {
  const rel = repoRel(f)
  const parts = rel.split('/')
  // Find the feature directory
  let feature = 'root'
  const featIdx = parts.indexOf('features')
  if (featIdx >= 0) {
    feature = parts[featIdx + 1] || 'features'
  } else if (rel.includes('shared/')) {
    feature = 'shared'
  } else if (rel.includes('app/')) {
    feature = 'app'
  }
  if (!storybookByFeature.has(feature)) storybookByFeature.set(feature, [])
  storybookByFeature.get(feature).push(rel)
}

// ── Duplicate contract files in same crate ──

const contractDuplicates = []
const crateTestDirMap = new Map()
for (const f of [...crateTestFiles, ...tauriTestFiles]) {
  const dir = dirname(f)
  if (!crateTestDirMap.has(dir)) crateTestDirMap.set(dir, [])
  crateTestDirMap.get(dir).push(f)
}
for (const [dir, files] of crateTestDirMap) {
  const hasContract = files.some((f) => basename(f) === 'contract.rs')
  const hasApiContract = files.some((f) => basename(f) === 'api_contract.rs')
  if (hasContract && hasApiContract) {
    contractDuplicates.push(repoRel(dir))
  }
}

// ── Output ──

console.log('# Jyowo Test Inventory')
console.log()
console.log('## Totals by Layer')
console.log()
console.log(`| Layer | Count |`)
console.log(`|---|---|`)
console.log(`| Frontend Vitest files | ${frontendTestFiles.length} |`)
console.log(`| Frontend Vitest test cases | ${frontendTestCases} |`)
console.log(`| Storybook files | ${storybookFiles.length} |`)
console.log(`| Playwright spec files | ${playwrightFiles.length} |`)
console.log(`| Rust test files | ${allRustTestFiles.length} |`)
console.log(`| Rust \`#[test]\` / \`#[tokio::test]\` count | ${rustTestFnCount} |`)
console.log(`| Script policy test files | ${scriptTestFiles.length} |`)
console.log()

console.log('## Largest Test Files by Line Count')
console.log()
console.log('| File | Lines | Kind |')
console.log('|---|---|---|')
for (const f of fileLineCounts.slice(0, 30)) {
  console.log(`| ${repoRel(f.path)} | ${f.lines} | ${f.kind} |`)
}
console.log()

console.log('## Files Over 1200 Lines (hard fail)')
console.log()
if (over1200.length === 0) {
  console.log('None.')
} else {
  for (const f of over1200) {
    console.log(`- ${repoRel(f.path)} (${f.lines} lines)`)
  }
}
console.log()

console.log('## Files Over 800 Lines (warning)')
console.log()
if (over800.length === 0) {
  console.log('None.')
} else {
  for (const f of over800) {
    console.log(`- ${repoRel(f.path)} (${f.lines} lines)`)
  }
}
console.log()

console.log('## Disallowed or Suspect Names')
console.log()
if (disallowedNames.length === 0) {
  console.log('None.')
} else {
  for (const f of disallowedNames) {
    console.log(`- ${repoRel(f.path)}`)
  }
}
console.log()

console.log('## Ignored / Manual / Live / Stress Tests')
console.log()
console.log('### Ignored tests')
for (const t of ignoredTests) {
  console.log(`- ${repoRel(t.path)}`)
}
console.log()
console.log('### manual_live_*.rs')
for (const t of manualLiveTests) {
  const status = t.ignored ? 'ignored' : 'NOT IGNORED'
  console.log(`- ${repoRel(t.path)} (${status})`)
}
if (manualLiveTests.length === 0) console.log('None.')
console.log()
console.log('### stress_*.rs')
for (const t of stressTests) {
  const status = t.ignored ? 'ignored' : 'NOT IGNORED'
  console.log(`- ${repoRel(t.path)} (${status})`)
}
if (stressTests.length === 0) console.log('None.')
console.log()

console.log('## createTestCommandClient Usage by File')
console.log()
for (const [file, count] of cmdClientUsageSorted) {
  console.log(`- ${file} (${count})`)
}
console.log()

console.log('## Storybook Files by Feature')
console.log()
for (const [feature, files] of [...storybookByFeature.entries()].sort()) {
  console.log(`### ${feature}`)
  for (const f of files) {
    console.log(`- ${f}`)
  }
  console.log()
}

console.log('## Duplicate contract.rs / api_contract.rs Pairs')
console.log()
if (contractDuplicates.length === 0) {
  console.log('None.')
} else {
  for (const dir of contractDuplicates) {
    console.log(`- ${dir}`)
  }
}
