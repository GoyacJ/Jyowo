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

  it('keeps the conversation canvas on projected turns instead of raw event blocks', () => {
    const files = [
      'src/features/conversation/timeline/pending-tool-permission.ts',
      'src/features/conversation/timeline/conversation-timeline-selectors.ts',
      'src/features/conversation/timeline/conversation-timeline-store.ts',
      'src/features/conversation/timeline/use-conversation-timeline.ts',
      'src/features/conversation/timeline/conversation-timeline.tsx',
    ]
    const source = files.map((file) => readFileSync(join(process.cwd(), file), 'utf8')).join('\n')

    expect(source).not.toContain('RunEvent')
    expect(source).not.toContain('PermissionRequestBlock')
    expect(source).not.toContain("kind: 'permissionRequest'")
    expect(source).not.toContain('Tool error withheld from conversation timeline')
    expect(source).not.toContain('get_conversation.messages')
    expect(source).toContain('ConversationTurn')
    expect(source).toContain('pageConversationWorktree')
  })

  it('keeps production timeline APIs named as turns instead of blocks', () => {
    const files = [
      'src/shared/state/ui-store.ts',
      'src/features/conversation/timeline/conversation-timeline.tsx',
      'src/features/conversation/timeline/conversation-turn-row.tsx',
      'src/features/conversation/timeline/conversation-timeline-selectors.ts',
      'src/features/conversation/timeline/use-conversation-timeline.ts',
      'src/features/conversation/ConversationWorkspace.tsx',
    ]
    const source = files.map((file) => readFileSync(join(process.cwd(), file), 'utf8')).join('\n')

    expect(source).not.toContain('ConversationBlockRow')
    expect(source).not.toContain('blocks?: ConversationTurn[]')
    expect(source).not.toContain('pendingPermissionBlocks')
    expect(source).not.toContain('blockId')
    expect(source).not.toContain('conversation-block-')
  })
})
