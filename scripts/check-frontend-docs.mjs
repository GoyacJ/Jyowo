import { readdirSync, readFileSync } from 'node:fs'
import {
  dirname,
  join,
  normalize,
  relative as relativePath,
  sep,
} from 'node:path'
import { fileURLToPath } from 'node:url'

const repoRoot = dirname(dirname(fileURLToPath(import.meta.url)))
const docsDir = join(repoRoot, 'docs', 'frontend')
const desktopSrcDir = join(repoRoot, 'apps', 'desktop', 'src')

const requiredDocs = [
  'agent-harness-frontend-development-guidelines.md',
  'frontend-product-ux.md',
  'frontend-engineering.md',
  'frontend-quality.md',
]

const requiredConcepts = [
  'ToolCallCard',
  'PermissionDialog',
  'DiffViewer',
  'CommandPreview',
  'Raw JSON',
  'TanStack Virtual',
  '@chenglou/pretext',
  'RunEvent',
  'CommandClient',
  'MCP',
  'Memory',
  'ProviderSettings',
  'Replay',
  'Audit',
  'RiskLevel',
  'Secret',
  'Storybook',
  'Biome',
  'Knip',
  'Playwright',
  'Zod',
  'Zustand',
  'React Hook Form',
]
const forbiddenStageLanguage = [
  'DEFERRED',
  'current phase',
  'target phase',
  'first phase',
  'foundation phase',
  'future',
  'deferred',
  'MVP',
  'P0',
  'P1',
  'P2',
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

function quotedValues(text) {
  return [...text.matchAll(/'([^']+)'/g)].map((match) => match[1])
}

function relative(path) {
  return path.slice(repoRoot.length + 1)
}

const frontendDocEntries = readdirSync(docsDir, { withFileTypes: true })
const markdownFiles = frontendDocEntries
  .filter((entry) => entry.isFile() && entry.name.endsWith('.md'))
  .map((entry) => entry.name)
  .sort()

const missingDocs = requiredDocs.filter((file) => !markdownFiles.includes(file))
const unexpectedDocs = frontendDocEntries
  .filter((entry) => entry.isDirectory() || !requiredDocs.includes(entry.name))
  .map((entry) => (entry.isDirectory() ? `${entry.name}/` : entry.name))
  .sort()

const activeDocs = markdownFiles
  .map((file) => read(join(docsDir, file)))
  .join('\n')

const oldNameMatches = activeDocs.match(/octo[p]us|Octo[p]us|OCTO[P]US|\/data\/octo[p]us/g) ?? []
const missingConcepts = requiredConcepts.filter((concept) => !activeDocs.includes(concept))
const stageLanguageMatches = forbiddenStageLanguage.filter((phrase) =>
  activeDocs.includes(phrase),
)
const engineeringDoc = read(join(docsDir, 'frontend-engineering.md'))
const runEventSourceDocMatch = engineeringDoc.match(/source:\s*((?:'[^']+'\s*\|\s*)*'[^']+')/)
const runEventSourceCode = read(
  join(desktopSrcDir, 'shared', 'events', 'run-event-schema.ts'),
)
const runEventSourceCodeMatch = runEventSourceCode.match(
  /runEventSourceSchema\s*=\s*z\.enum\(\[([^\]]+)\]\)/,
)
const documentedRunEventSources = runEventSourceDocMatch
  ? quotedValues(runEventSourceDocMatch[1])
  : []
const implementedRunEventSources = runEventSourceCodeMatch
  ? quotedValues(runEventSourceCodeMatch[1])
  : []
const runEventSourceMismatch =
  documentedRunEventSources.length === 0 ||
  implementedRunEventSources.length === 0 ||
  documentedRunEventSources.join('\n') !== implementedRunEventSources.join('\n')

const sourceFiles = listFiles(desktopSrcDir, ['.ts', '.tsx']).filter(
  (file) => !file.endsWith('routeTree.gen.ts'),
)

function importSpecifiers(text) {
  return [
    ...text.matchAll(
      /(?:import|export)\s+(?:type\s+)?(?:[^'"]*?\s+from\s+)?['"]([^'"]+)['"]/g,
    ),
    ...text.matchAll(/import\s*\(\s*['"]([^'"]+)['"]\s*\)/g),
  ].map((match) => match[1])
}

function sourceLayer(path) {
  const [layer] = relativePath(desktopSrcDir, path).split(sep)

  return ['app', 'routes', 'features', 'shared'].includes(layer) ? layer : null
}

function importLayer(fromFile, specifier) {
  if (specifier.startsWith('@/')) {
    const [, layer] = specifier.split('/')
    return ['app', 'routes', 'features', 'shared'].includes(layer) ? layer : null
  }

  if (!specifier.startsWith('.')) {
    return null
  }

  const resolvedPath = normalize(join(dirname(fromFile), specifier))
  if (!resolvedPath.startsWith(`${desktopSrcDir}${sep}`)) {
    return null
  }

  return sourceLayer(resolvedPath)
}

const bareTauriImports = sourceFiles.filter((file) => {
  return !file.includes(join('shared', 'tauri')) && read(file).includes('@tauri-apps/api')
})
const directPretextImports = sourceFiles.filter((file) => {
  return !file.includes(join('shared', 'text-layout')) && read(file).includes('@chenglou/pretext')
})
const sharedReverseImports = sourceFiles.filter((file) => {
  return (
    file.includes(join('src', 'shared')) &&
    /from ['"]@\/(?:app|routes|features)\//.test(read(file))
  )
})
const forbiddenLayerImports = sourceFiles.flatMap((file) => {
  const fromLayer = sourceLayer(file)

  if (!fromLayer) {
    return []
  }

  return importSpecifiers(read(file)).flatMap((specifier) => {
    const toLayer = importLayer(file, specifier)
    const isForbidden =
      (fromLayer === 'shared' && ['app', 'routes', 'features'].includes(toLayer ?? '')) ||
      (fromLayer === 'features' && ['app', 'routes'].includes(toLayer ?? ''))

    if (!isForbidden) {
      return []
    }

    return [
      {
        file,
        specifier,
        fromLayer,
        toLayer,
      },
    ]
  })
})

if (
  missingDocs.length > 0 ||
  unexpectedDocs.length > 0 ||
  oldNameMatches.length > 0 ||
  stageLanguageMatches.length > 0 ||
  missingConcepts.length > 0 ||
  runEventSourceMismatch ||
  bareTauriImports.length > 0 ||
  directPretextImports.length > 0 ||
  sharedReverseImports.length > 0 ||
  forbiddenLayerImports.length > 0
) {
  console.error('Frontend docs check failed.')
  if (missingDocs.length > 0) {
    console.error('\nMissing active docs:')
    for (const file of missingDocs) {
      console.error(`- ${file}`)
    }
  }
  if (unexpectedDocs.length > 0) {
    console.error('\nUnexpected frontend docs:')
    for (const file of unexpectedDocs) {
      console.error(`- ${file}`)
    }
  }
  if (oldNameMatches.length > 0) {
    console.error('\nOld project names found in active frontend docs.')
  }
  if (stageLanguageMatches.length > 0) {
    console.error('\nStage-based language found in normative frontend docs:')
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
  if (runEventSourceMismatch) {
    console.error('\nRunEvent source values do not match frontend-engineering.md.')
    console.error(`Documented: ${documentedRunEventSources.join(', ') || '(not found)'}`)
    console.error(`Implemented: ${implementedRunEventSources.join(', ') || '(not found)'}`)
  }
  if (bareTauriImports.length > 0) {
    console.error('\nBare Tauri API imports outside shared/tauri:')
    for (const file of bareTauriImports) {
      console.error(`- ${relative(file)}`)
    }
  }
  if (directPretextImports.length > 0) {
    console.error('\nDirect pretext imports outside shared/text-layout:')
    for (const file of directPretextImports) {
      console.error(`- ${relative(file)}`)
    }
  }
  if (sharedReverseImports.length > 0) {
    console.error('\nshared must not import app/routes/features:')
    for (const file of sharedReverseImports) {
      console.error(`- ${relative(file)}`)
    }
  }
  if (forbiddenLayerImports.length > 0) {
    console.error('\nForbidden layer imports:')
    for (const violation of forbiddenLayerImports) {
      console.error(
        `- ${relative(violation.file)} imports ${violation.toLayer} via ${violation.specifier}`,
      )
    }
  }
  process.exit(1)
}

console.log(`Frontend docs check passed: ${requiredDocs.length} active docs verified.`)
