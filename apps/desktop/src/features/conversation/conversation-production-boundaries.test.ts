import { existsSync } from 'node:fs'
import { join } from 'node:path'
import { describe, expect, it } from 'vitest'

describe('conversation production boundaries', () => {
  it('keeps the retired conversation workspace and event stream out of the desktop bundle', () => {
    const retiredModules = [
      'src/features/conversation/ConversationWorkspace.tsx',
      'src/features/conversation/timeline/conversation-timeline-source.ts',
      'src/features/conversation/timeline/conversation-timeline-store.ts',
      'src/features/conversation/timeline/conversation-timeline-selectors.ts',
      'src/features/conversation/timeline/use-conversation-event-stream.ts',
      'src/features/conversation/timeline/use-conversation-timeline.ts',
    ]

    for (const retiredModule of retiredModules) {
      expect(existsSync(join(process.cwd(), retiredModule)), retiredModule).toBe(false)
    }
  })
})
