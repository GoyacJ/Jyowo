import { readFileSync } from 'node:fs'
import { join } from 'node:path'
import { describe, expect, it } from 'vitest'

describe('conversation production boundaries', () => {
  it('keeps the production workspace off the mock conversation runtime', () => {
    const source = readFileSync(
      join(process.cwd(), 'src/features/conversation/ConversationWorkspace.tsx'),
      'utf8',
    )

    expect(source).not.toContain('mock-conversation-runtime')
    expect(source).not.toContain('mockConversationRuntime')
    expect(source).not.toContain('createMockConversationState')
  })
})
