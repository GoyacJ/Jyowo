import { spawnSync } from 'node:child_process'
import { existsSync, readdirSync, readFileSync } from 'node:fs'
import { dirname, join } from 'node:path'
import { fileURLToPath } from 'node:url'
import {
  linesFromTextBlock,
  normalizeMarkdownTableCell,
  registeredTauriCommands,
  rustDependencyPolicyRows,
  tauriCommandNames,
  tomlSection,
  workspaceDependencyLayerViolations,
  workspaceLayerRows,
  workspaceMembers,
} from './backend-docs-policy.mjs'
import { upstreamHeldRustDependencies } from './rust-deps-policy.mjs'

const repoRoot = dirname(dirname(fileURLToPath(import.meta.url)))
const docsDir = join(repoRoot, 'docs', 'backend')
const rootCargoPath = join(repoRoot, 'Cargo.toml')
const tauriSrcDir = join(repoRoot, 'apps', 'desktop', 'src-tauri', 'src')
const tauriLibPath = join(repoRoot, 'apps', 'desktop', 'src-tauri', 'src', 'lib.rs')

const requiredDocs = [
  'agent-harness-backend-development-guidelines.md',
  'backend-runtime.md',
  'backend-engineering.md',
  'backend-quality.md',
]

const requiredArchitectureDocs = [
  'docs/architecture/harness/crates/harness-model.md',
  'docs/architecture/harness/crates/harness-plugin.md',
]

const requiredConcepts = [
  'Policy authority',
  'PermissionBroker',
  'Event',
  'Journal',
  'Redactor',
  'Secret',
  'Tauri command',
  'harness-contracts',
  'Run',
  'Tool',
  'MCP',
  'Memory',
  'Model',
  'Replay',
  'Audit',
  'fail-closed',
  'fail-open',
  'unsafe_code',
  'serde',
  'JsonSchema',
  'cargo fmt',
  'cargo check',
  'cargo test',
  'cargo update --dry-run',
  'check:rust-deps',
  'generic-array',
  'matchit',
  'toml',
  'system-deps',
  'crypto-common',
  'proc-macro-crate',
  'toml_datetime',
  'toml_edit',
  'CapabilityRouteKind',
  'ToolServiceBinding',
  'provider-capability-routes',
]

const forbiddenStageLanguage = [
  { label: 'DEFERRED', pattern: /\bdeferred\b/i },
  { label: 'current phase', pattern: /\bcurrent\s+phase\b/i },
  { label: 'target phase', pattern: /\btarget\s+phase\b/i },
  { label: 'first phase', pattern: /\bfirst\s+phase\b/i },
  { label: 'foundation phase', pattern: /\bfoundation\s+phase\b/i },
  { label: 'future', pattern: /\bfuture\b/i },
  { label: 'MVP', pattern: /\bmvp\b/i },
  { label: 'P0', pattern: /\bP0\b/ },
  { label: 'P1', pattern: /\bP1\b/ },
  { label: 'P2', pattern: /\bP2\b/ },
  { label: 'phase number', pattern: /\bphase\s+\d+\b/i },
  { label: 'stage number', pattern: /\bstage\s+\d+\b/i },
]

const expectedLayers = new Map([
  ['jyowo-desktop-shell', 'Tauri shell'],
  ['jyowo-harness-contracts', 'L0'],
  ['jyowo-harness-budget', 'L1'],
  ['jyowo-harness-journal', 'L1'],
  ['jyowo-harness-memory', 'L1'],
  ['jyowo-harness-model', 'L1'],
  ['jyowo-harness-permission', 'L1'],
  ['jyowo-harness-sandbox', 'L1'],
  ['jyowo-harness-context', 'L2'],
  ['jyowo-harness-hook', 'L2'],
  ['jyowo-harness-mcp', 'L2'],
  ['jyowo-harness-session', 'L2'],
  ['jyowo-harness-skill', 'L2'],
  ['jyowo-harness-tool', 'L2'],
  ['jyowo-harness-tool-search', 'L2'],
  ['jyowo-harness-engine', 'L3'],
  ['jyowo-harness-observability', 'L3'],
  ['jyowo-harness-plugin', 'L3'],
  ['jyowo-harness-subagent', 'L3'],
  ['jyowo-harness-team', 'L3'],
  ['jyowo-harness-sdk', 'L4'],
])

const requiredCommandNames = ['get_app_info', 'harness_healthcheck']

const criticalTests = [
  'apps/desktop/src-tauri/tests/commands.rs',
  'crates/jyowo-harness-budget/tests/budget_contract.rs',
  'crates/jyowo-harness-contracts/tests/m1_contracts.rs',
  'crates/jyowo-harness-journal/tests/version.rs',
  'crates/jyowo-harness-observability/tests/journal_redactor_pipeline.rs',
  'crates/jyowo-harness-sdk/tests/runtime_assembly.rs',
  'crates/jyowo-harness-tool/tests/builtin_exec.rs',
  'crates/jyowo-harness-tool/tests/builtin_io.rs',
  'crates/jyowo-harness-tool/tests/orchestrator.rs',
]

function read(path) {
  return readFileSync(path, 'utf8')
}

function listFiles(dir, extensions) {
  return readdirSync(dir, { withFileTypes: true }).flatMap((entry) => {
    const path = join(dir, entry.name)

    if (entry.isDirectory()) {
      return listFiles(path, extensions)
    }

    return extensions.some((extension) => path.endsWith(extension)) ? [path] : []
  })
}

function packageNameForMember(member) {
  const cargoPath = join(repoRoot, member, 'Cargo.toml')

  if (!existsSync(cargoPath)) {
    return null
  }

  return read(cargoPath).match(/^\s*name\s*=\s*"([^"]+)"/m)?.[1] ?? null
}

let markdownFiles = []
let missingDocs = [...requiredDocs]
let missingArchitectureDocs = []
let unexpectedDocs = []
let activeDocs = ''

if (existsSync(docsDir)) {
  const backendDocEntries = readdirSync(docsDir, { withFileTypes: true })
  markdownFiles = backendDocEntries
    .filter((entry) => entry.isFile() && entry.name.endsWith('.md'))
    .map((entry) => entry.name)
    .sort()

  missingDocs = requiredDocs.filter((file) => !markdownFiles.includes(file))
  unexpectedDocs = backendDocEntries
    .filter((entry) => entry.isDirectory() || !requiredDocs.includes(entry.name))
    .map((entry) => (entry.isDirectory() ? `${entry.name}/` : entry.name))
    .sort()

  activeDocs = markdownFiles.map((file) => read(join(docsDir, file))).join('\n')
}

missingArchitectureDocs = requiredArchitectureDocs.filter((path) => !existsSync(join(repoRoot, path)))

const engineeringDocPath = join(docsDir, 'backend-engineering.md')
const qualityDocPath = join(docsDir, 'backend-quality.md')
const engineeringDoc = existsSync(engineeringDocPath) ? read(engineeringDocPath) : ''
const qualityDoc = existsSync(qualityDocPath) ? read(qualityDocPath) : ''
const rootCargo = read(rootCargoPath)
const cargoMetadataResult = spawnSync('cargo', ['metadata', '--format-version', '1'], {
  cwd: repoRoot,
  encoding: 'utf8',
  maxBuffer: 128 * 1024 * 1024,
})
const cargoMetadata =
  cargoMetadataResult.status === 0 && cargoMetadataResult.stdout
    ? JSON.parse(cargoMetadataResult.stdout)
    : null
const tauriCommandSources = listFiles(tauriSrcDir, ['.rs']).map(read)
const tauriLibSource = read(tauriLibPath)

const oldNameMatches = activeDocs.match(/octo[p]us|Octo[p]us|OCTO[P]US|\/data\/octo[p]us/g) ?? []
const stageLanguageMatches = forbiddenStageLanguage
  .filter(({ pattern }) => pattern.test(activeDocs))
  .map(({ label }) => label)
const missingConcepts = requiredConcepts.filter((concept) => !activeDocs.includes(concept))

const implementedCommands = tauriCommandNames(tauriCommandSources)
const registeredCommands = registeredTauriCommands(tauriLibSource)
const documentedCommands = linesFromTextBlock(engineeringDoc, 'Current Tauri commands')
const unregisteredCommands = implementedCommands.filter((command) => !registeredCommands.includes(command))
const registeredNonCommandHandlers = registeredCommands.filter((command) => !implementedCommands.includes(command))
const undocumentedCommands = implementedCommands.filter((command) => !documentedCommands.includes(command))
const staleDocumentedCommands = documentedCommands.filter((command) => !implementedCommands.includes(command))
const missingRequiredCommands = requiredCommandNames.filter(
  (command) => !implementedCommands.includes(command) || !documentedCommands.includes(command),
)

const members = workspaceMembers(rootCargo)
const workspacePackages = members.map((member) => ({
  member,
  packageName: packageNameForMember(member),
}))
const missingWorkspacePackageNames = workspacePackages.filter((entry) => entry.packageName === null)
const layerRows = workspaceLayerRows(engineeringDoc)
const layerRowsByPackage = new Map(layerRows.map((row) => [row.packageName, row]))
const expectedPackageNames = workspacePackages
  .map((entry) => entry.packageName)
  .filter((packageName) => packageName !== null)
const packagesWithoutExpectedLayer = workspacePackages.filter(
  (entry) => entry.packageName !== null && !expectedLayers.has(entry.packageName),
)
const missingLayerRows = workspacePackages.filter((entry) => {
  if (entry.packageName === null) {
    return true
  }

  const row = layerRowsByPackage.get(entry.packageName)
  return row === undefined || row.path !== entry.member
})
const unexpectedLayerRows = layerRows.filter((row) => !expectedPackageNames.includes(row.packageName))
const layerMismatches = workspacePackages.flatMap((entry) => {
  if (entry.packageName === null) {
    return []
  }

  const row = layerRowsByPackage.get(entry.packageName)
  const expectedLayer = expectedLayers.get(entry.packageName)

  if (!row || expectedLayer === undefined || row.layer === expectedLayer) {
    return []
  }

  return [
    {
      packageName: entry.packageName,
      expectedLayer,
      documentedLayer: row.layer,
    },
  ]
})
const workspaceDependencyViolations =
  cargoMetadata === null
    ? []
    : workspaceDependencyLayerViolations(cargoMetadata, Object.fromEntries(expectedLayers))
const undocumentedCriticalTests = criticalTests.filter((path) => !activeDocs.includes(path))
const missingCriticalTestFiles = criticalTests.filter((path) => !existsSync(join(repoRoot, path)))
const rustLintSection = tomlSection(rootCargo, 'workspace.lints.rust')
const unsafeLintMissing = !/^\s*unsafe_code\s*=\s*"forbid"\s*$/m.test(rustLintSection)
const unsafeLintUndocumented = !activeDocs.includes('unsafe_code = "forbid"')
const rustDependencyRows = rustDependencyPolicyRows(qualityDoc)
const rustDependencyRowsByName = new Map(rustDependencyRows.map((row) => [row.name, row]))
const missingRustDependencyRows = upstreamHeldRustDependencies.filter(
  (dependency) => !rustDependencyRowsByName.has(dependency.name),
)
const staleRustDependencyRows = rustDependencyRows.filter(
  (row) => !upstreamHeldRustDependencies.some((dependency) => dependency.name === row.name),
)
const mismatchedRustDependencyRows = upstreamHeldRustDependencies.flatMap((dependency) => {
  const row = rustDependencyRowsByName.get(dependency.name)

  if (!row) {
    return []
  }

  const expected = {
    current: dependency.current,
    available: dependency.available,
    owner: dependency.owner,
    constraint: normalizeMarkdownTableCell(dependency.constraint),
  }

  const actual = {
    current: row.current,
    available: row.available,
    owner: row.owner,
    constraint: normalizeMarkdownTableCell(row.constraint),
  }

  return Object.entries(expected)
    .filter(([field, value]) => actual[field] !== value)
    .map(([field, value]) => ({
      name: dependency.name,
      field,
      expected: value,
      actual: actual[field],
    }))
})

if (
  missingDocs.length > 0 ||
  missingArchitectureDocs.length > 0 ||
  unexpectedDocs.length > 0 ||
  oldNameMatches.length > 0 ||
  stageLanguageMatches.length > 0 ||
  missingConcepts.length > 0 ||
  unregisteredCommands.length > 0 ||
  registeredNonCommandHandlers.length > 0 ||
  undocumentedCommands.length > 0 ||
  staleDocumentedCommands.length > 0 ||
  missingRequiredCommands.length > 0 ||
  missingWorkspacePackageNames.length > 0 ||
  packagesWithoutExpectedLayer.length > 0 ||
  missingLayerRows.length > 0 ||
  unexpectedLayerRows.length > 0 ||
  layerMismatches.length > 0 ||
  workspaceDependencyViolations.length > 0 ||
  undocumentedCriticalTests.length > 0 ||
  missingCriticalTestFiles.length > 0 ||
  cargoMetadataResult.status !== 0 ||
  unsafeLintMissing ||
  unsafeLintUndocumented ||
  missingRustDependencyRows.length > 0 ||
  staleRustDependencyRows.length > 0 ||
  mismatchedRustDependencyRows.length > 0
) {
  console.error('Backend docs check failed.')
  if (missingDocs.length > 0) {
    console.error('\nMissing active docs:')
    for (const file of missingDocs) {
      console.error(`- ${file}`)
    }
  }
  if (missingArchitectureDocs.length > 0) {
    console.error('\nMissing architecture docs:')
    for (const file of missingArchitectureDocs) {
      console.error(`- ${file}`)
    }
  }
  if (unexpectedDocs.length > 0) {
    console.error('\nUnexpected backend docs:')
    for (const file of unexpectedDocs) {
      console.error(`- ${file}`)
    }
  }
  if (oldNameMatches.length > 0) {
    console.error('\nOld project names found in active backend docs.')
  }
  if (stageLanguageMatches.length > 0) {
    console.error('\nStage-based language found in normative backend docs:')
    for (const phrase of stageLanguageMatches) {
      console.error(`- ${phrase}`)
    }
  }
  if (missingConcepts.length > 0) {
    console.error('\nMissing required concepts:')
    for (const concept of missingConcepts) {
      console.error(`- ${concept}`)
    }
  }
  if (unregisteredCommands.length > 0) {
    console.error('\nTauri commands missing from generate_handler!:')
    for (const command of unregisteredCommands) {
      console.error(`- ${command}`)
    }
  }
  if (registeredNonCommandHandlers.length > 0) {
    console.error('\nRegistered Tauri handlers missing #[tauri::command]:')
    for (const command of registeredNonCommandHandlers) {
      console.error(`- ${command}`)
    }
  }
  if (undocumentedCommands.length > 0) {
    console.error('\nImplemented Tauri commands missing from backend-engineering.md:')
    for (const command of undocumentedCommands) {
      console.error(`- ${command}`)
    }
  }
  if (staleDocumentedCommands.length > 0) {
    console.error('\nDocumented Tauri commands not implemented:')
    for (const command of staleDocumentedCommands) {
      console.error(`- ${command}`)
    }
  }
  if (missingRequiredCommands.length > 0) {
    console.error('\nRequired Tauri commands missing from implementation or docs:')
    for (const command of missingRequiredCommands) {
      console.error(`- ${command}`)
    }
  }
  if (missingWorkspacePackageNames.length > 0) {
    console.error('\nWorkspace members without readable package names:')
    for (const entry of missingWorkspacePackageNames) {
      console.error(`- ${entry.member}`)
    }
  }
  if (packagesWithoutExpectedLayer.length > 0) {
    console.error('\nWorkspace packages missing expected layer rules:')
    for (const entry of packagesWithoutExpectedLayer) {
      console.error(`- ${entry.packageName} at ${entry.member}`)
    }
  }
  if (missingLayerRows.length > 0) {
    console.error('\nWorkspace packages missing from backend-engineering.md layer table:')
    for (const entry of missingLayerRows) {
      console.error(`- ${entry.packageName ?? '(unknown)'} at ${entry.member}`)
    }
  }
  if (unexpectedLayerRows.length > 0) {
    console.error('\nLayer table rows not present in Cargo workspace:')
    for (const row of unexpectedLayerRows) {
      console.error(`- ${row.packageName} at ${row.path}`)
    }
  }
  if (layerMismatches.length > 0) {
    console.error('\nLayer table mismatches:')
    for (const mismatch of layerMismatches) {
      console.error(
        `- ${mismatch.packageName}: documented ${mismatch.documentedLayer}, expected ${mismatch.expectedLayer}`,
      )
    }
  }
  if (workspaceDependencyViolations.length > 0) {
    console.error('\nWorkspace dependency layer violations:')
    for (const violation of workspaceDependencyViolations) {
      console.error(
        `- ${violation.packageName} (${violation.packageLayer}) depends on ${violation.dependencyName} (${violation.dependencyLayer})`,
      )
    }
  }
  if (cargoMetadataResult.status !== 0) {
    console.error('\nUnable to read cargo metadata:')
    process.stderr.write(cargoMetadataResult.stderr)
  }
  if (undocumentedCriticalTests.length > 0) {
    console.error('\nCritical backend tests missing from backend docs:')
    for (const path of undocumentedCriticalTests) {
      console.error(`- ${path}`)
    }
  }
  if (missingCriticalTestFiles.length > 0) {
    console.error('\nCritical backend test files do not exist:')
    for (const path of missingCriticalTestFiles) {
      console.error(`- ${path}`)
    }
  }
  if (unsafeLintMissing) {
    console.error('\nCargo workspace must forbid unsafe_code.')
  }
  if (unsafeLintUndocumented) {
    console.error('\nBackend docs must document unsafe_code = "forbid".')
  }
  if (missingRustDependencyRows.length > 0) {
    console.error('\nRust dependency policy rows missing from backend-quality.md:')
    for (const dependency of missingRustDependencyRows) {
      console.error(`- ${dependency.name}`)
    }
  }
  if (staleRustDependencyRows.length > 0) {
    console.error('\nStale Rust dependency policy rows in backend-quality.md:')
    for (const row of staleRustDependencyRows) {
      console.error(`- ${row.name}`)
    }
  }
  if (mismatchedRustDependencyRows.length > 0) {
    console.error('\nRust dependency policy row mismatches in backend-quality.md:')
    for (const mismatch of mismatchedRustDependencyRows) {
      console.error(
        `- ${mismatch.name}.${mismatch.field}: documented ${mismatch.actual}, expected ${mismatch.expected}`,
      )
    }
  }
  process.exit(1)
}

console.log(`Backend docs check passed: ${requiredDocs.length} active docs verified.`)
