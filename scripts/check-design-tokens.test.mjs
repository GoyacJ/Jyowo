import assert from 'node:assert/strict'
import { randomUUID } from 'node:crypto'
import { mkdirSync, writeFileSync } from 'node:fs'
import { join } from 'node:path'
import { tmpdir } from 'node:os'
import test from 'node:test'

import { scanDesignTokenUsage } from './check-design-tokens.mjs'

function writeFixture(files) {
  const root = join(tmpdir(), `jyowo-design-token-check-${randomUUID()}`)
  for (const [relativePath, content] of Object.entries(files)) {
    const absolutePath = join(root, relativePath)
    mkdirSync(join(absolutePath, '..'), { recursive: true })
    writeFileSync(absolutePath, content, 'utf8')
  }
  return root
}

test('warns on direct palette classes and arbitrary shadows in production UI code', () => {
  const root = writeFixture({
    'apps/desktop/src/features/demo/Demo.tsx': `
export function Demo() {
  return <div className="bg-slate-900 text-blue-100 shadow-[0_1px_2px_rgba(0,0,0,0.1)]" />
}
`,
  })

  const result = scanDesignTokenUsage(root)

  assert.equal(result.ok, true)
  assert.equal(result.warnings.length, 3)
  assert.deepEqual(
    result.warnings.map((warning) => warning.rule),
    ['tailwind-palette-class', 'tailwind-palette-class', 'arbitrary-shadow'],
  )
})

test('allows semantic token classes and test files', () => {
  const root = writeFixture({
    'apps/desktop/src/features/demo/Demo.tsx': `
export function Demo() {
  return <div className="bg-surface text-muted-foreground border-border shadow-card" />
}
`,
    'apps/desktop/src/features/demo/Demo.test.tsx': `
test('fixture may mention bg-slate-900', () => {})
`,
  })

  const result = scanDesignTokenUsage(root)

  assert.equal(result.ok, true)
  assert.equal(result.warnings.length, 0)
})
