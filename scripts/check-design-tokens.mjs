import { existsSync, readdirSync, readFileSync, statSync } from 'node:fs'
import { dirname, join, relative } from 'node:path'
import { fileURLToPath, pathToFileURL } from 'node:url'

const defaultRepoRoot = dirname(dirname(fileURLToPath(import.meta.url)))

const SCOPED_PATHS = ['apps/desktop/src']
const EXCLUDED_PATH_SEGMENTS = ['node_modules', 'dist', 'storybook-static']
const EXCLUDED_FILE_SUFFIXES = [
  '.test.ts',
  '.test.tsx',
  '.stories.ts',
  '.stories.tsx',
  '.gen.ts',
]
const EXCLUDED_FILES = new Set(['apps/desktop/src/shared/styles/global.css'])

const CHECKED_EXTENSIONS = ['.ts', '.tsx', '.css']

const RULES = [
  {
    id: 'tailwind-palette-class',
    pattern:
      /\b(?:bg|text|border|ring|from|to|via|fill|stroke)-(?:slate|gray|zinc|neutral|stone|red|orange|amber|yellow|lime|green|emerald|teal|cyan|sky|blue|indigo|violet|purple|fuchsia|pink|rose|white|black)(?:[-/][A-Za-z0-9[\].%]+)?\b/g,
  },
  {
    id: 'arbitrary-shadow',
    pattern: /\bshadow-\[[^\]]+\]/g,
  },
  {
    id: 'direct-hex-color',
    pattern: /#[0-9a-fA-F]{3,8}\b/g,
  },
]

/** @typedef {{ file: string, line: number, rule: string, excerpt: string }} DesignTokenWarning */

/**
 * @param {string} repoRoot
 * @returns {{ ok: true, warnings: DesignTokenWarning[] }}
 */
export function scanDesignTokenUsage(repoRoot = defaultRepoRoot) {
  const files = collectFiles(repoRoot)
  /** @type {DesignTokenWarning[]} */
  const warnings = []

  for (const absolutePath of files) {
    const rel = relative(repoRoot, absolutePath)
    const lines = readFileSync(absolutePath, 'utf8').split(/\r?\n/)

    lines.forEach((line, index) => {
      for (const rule of RULES) {
        rule.pattern.lastIndex = 0
        const matches = line.matchAll(rule.pattern)
        for (const match of matches) {
          warnings.push({
            file: rel,
            line: index + 1,
            rule: rule.id,
            excerpt: match[0],
          })
        }
      }
    })
  }

  return { ok: true, warnings }
}

function collectFiles(repoRoot) {
  /** @type {string[]} */
  const files = []

  for (const scopedPath of SCOPED_PATHS) {
    const absolutePath = join(repoRoot, scopedPath)
    if (!existsSync(absolutePath)) {
      continue
    }
    walkDirectory(absolutePath, repoRoot, files)
  }

  return files.sort()
}

function walkDirectory(dir, repoRoot, files) {
  for (const entry of readdirSync(dir, { withFileTypes: true })) {
    const absolutePath = join(dir, entry.name)
    const rel = relative(repoRoot, absolutePath)

    if (entry.isDirectory()) {
      if (!EXCLUDED_PATH_SEGMENTS.includes(entry.name)) {
        walkDirectory(absolutePath, repoRoot, files)
      }
      continue
    }

    if (shouldScanFile(rel, absolutePath)) {
      files.push(absolutePath)
    }
  }
}

function shouldScanFile(rel, absolutePath) {
  if (EXCLUDED_FILES.has(rel)) {
    return false
  }
  if (EXCLUDED_FILE_SUFFIXES.some((suffix) => rel.endsWith(suffix))) {
    return false
  }
  if (!CHECKED_EXTENSIONS.some((extension) => rel.endsWith(extension))) {
    return false
  }
  return statSync(absolutePath).isFile()
}

export function reportDesignTokenWarnings(result = scanDesignTokenUsage()) {
  if (result.warnings.length === 0) {
    console.log('Design token usage check passed.')
    return
  }

  console.warn('Design token usage warnings:')
  for (const warning of result.warnings) {
    console.warn(`  ${warning.file}:${warning.line}: ${warning.rule}: ${warning.excerpt}`)
  }
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  reportDesignTokenWarnings()
}
