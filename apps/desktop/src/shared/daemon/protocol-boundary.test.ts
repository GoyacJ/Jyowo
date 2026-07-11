import { readdirSync, readFileSync } from 'node:fs'
import { join, relative } from 'node:path'

import { describe, expect, it } from 'vitest'

const sourceRoot = join(process.cwd(), 'src')
const allowedRoots = ['generated/', 'shared/daemon/']
const generatedSource = readFileSync(join(sourceRoot, 'generated/daemon-protocol.ts'), 'utf8')
const generatedProtocolNames = new Set(
  [...generatedSource.matchAll(/\b(?:type|interface)\s+([A-Z][A-Za-z0-9_]*)\b/g)].map(
    (match) => match[1] as string,
  ),
)

describe('daemon protocol boundary', () => {
  it('keeps daemon event declarations in generated output or shared daemon code', () => {
    const violations = sourceFiles(sourceRoot)
      .map((path) => ({ path, source: readFileSync(path, 'utf8') }))
      .filter(({ path }) => {
        const projectPath = relative(sourceRoot, path).replaceAll('\\', '/')
        return !allowedRoots.some((root) => projectPath.startsWith(root))
      })
      .filter(({ source }) => declaresGeneratedProtocol(source))
      .map(({ path }) => relative(process.cwd(), path).replaceAll('\\', '/'))

    expect(violations).toEqual([])
  })

  it('detects handwritten aliases and Zod schemas for generated declarations', () => {
    expect(declaresGeneratedProtocol('export interface TaskEventEnvelope {}')).toBe(true)
    expect(declaresGeneratedProtocol('const taskEventEnvelopeSchema = z.object({})')).toBe(true)
    expect(declaresGeneratedProtocol('const TaskEventEnvelopeSchema = z.object({})')).toBe(true)
    expect(declaresGeneratedProtocol('export type TaskDaemonEvent = unknown')).toBe(true)
    expect(declaresGeneratedProtocol('export const runProjectionSchema = z.object({})')).toBe(true)
    expect(declaresGeneratedProtocol('export type RunEvent = z.infer<typeof runEventSchema>')).toBe(
      false,
    )
  })
})

function declaresGeneratedProtocol(source: string) {
  for (const name of generatedProtocolNames) {
    const schemaNames = [`${name[0]?.toLowerCase()}${name.slice(1)}Schema`, `${name}Schema`]
    const declarationPattern = new RegExp(
      `\\b(?:type|interface)\\s+${name}\\b|\\b(?:const|let|var)\\s+(?:${schemaNames.join('|')})\\b`,
    )
    if (declarationPattern.test(source)) return true
  }
  return /\b(?:type|interface)\s+[A-Za-z0-9_]*DaemonEvent[A-Za-z0-9_]*\b|\b(?:const|let|var)\s+daemonEvent[A-Za-z0-9_]*Schema\b/.test(
    source,
  )
}

function sourceFiles(directory: string): string[] {
  return readdirSync(directory, { withFileTypes: true }).flatMap((entry) => {
    const path = join(directory, entry.name)
    if (entry.isDirectory()) return sourceFiles(path)
    return /\.(?:ts|tsx)$/.test(entry.name) && !entry.name.endsWith('.test.ts') ? [path] : []
  })
}
