import { readdirSync, readFileSync } from 'node:fs'
import { join, relative } from 'node:path'

import { describe, expect, it } from 'vitest'

const sourceRoot = join(process.cwd(), 'src')
const allowedRoots = ['generated/', 'shared/daemon/']
const daemonEventDeclaration =
  /\b(?:type|interface)\s+(?:DaemonEvent\w*|TaskEventEnvelope|EventBatch)\b/

describe('daemon protocol boundary', () => {
  it('keeps daemon event declarations in generated output or shared daemon code', () => {
    const violations = sourceFiles(sourceRoot)
      .map((path) => ({ path, source: readFileSync(path, 'utf8') }))
      .filter(({ path }) => {
        const projectPath = relative(sourceRoot, path).replaceAll('\\', '/')
        return !allowedRoots.some((root) => projectPath.startsWith(root))
      })
      .filter(({ source }) => daemonEventDeclaration.test(source))
      .map(({ path }) => relative(process.cwd(), path).replaceAll('\\', '/'))

    expect(violations).toEqual([])
  })
})

function sourceFiles(directory: string): string[] {
  return readdirSync(directory, { withFileTypes: true }).flatMap((entry) => {
    const path = join(directory, entry.name)
    if (entry.isDirectory()) return sourceFiles(path)
    return /\.(?:ts|tsx)$/.test(entry.name) && !entry.name.endsWith('.test.ts') ? [path] : []
  })
}
