import { readFileSync } from 'node:fs'
import { join } from 'node:path'
import { describe, expect, it } from 'vitest'

describe('conversation production boundaries', () => {
  it('keeps the production workspace off retired local fixtures and artifacts feature imports', () => {
    const source = readFileSync(
      join(process.cwd(), 'src/features/conversation/ConversationWorkspace.tsx'),
      'utf8',
    )
    const retiredRuntimeModule = ['mock', 'conversation', 'runtime'].join('-')
    const retiredFixtureModule = ['prototype', 'data'].join('-')
    const artifactsFeatureAlias = ['@/features', 'artifacts'].join('/')
    const oldRenderSources = [
      ['Conversation', 'Message'].join(''),
      ['Progress', 'Block'].join(''),
      ['Artifact', 'Summary'].join(''),
    ]

    expect(source).not.toContain(retiredRuntimeModule)
    expect(source).not.toContain('mockConversationRuntime')
    expect(source).not.toContain('createMockConversationState')
    expect(source).not.toContain(retiredFixtureModule)
    expect(source).not.toContain(artifactsFeatureAlias)
    for (const oldRenderSource of oldRenderSources) {
      expect(source).not.toContain(oldRenderSource)
    }
  })
})
