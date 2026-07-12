import type { CommandClient } from '@/shared/tauri/commands'

import { createAgentCommandHandlers } from './agents-handlers'
import type { TestCommandClientOptions } from './base'
import { createBaseCommandHandlers } from './base-handlers'
import { createMcpCommandHandlers } from './mcp-handlers'
import { createPluginCommandHandlers } from './plugins-handlers'
import { createSettingsCommandHandlers } from './settings-handlers'
import { createSkillCommandHandlers } from './skills-handlers'
import { createTestCommandClientState } from './state'

export type { TestCommandClientOptions } from './base'
export { testJyowoProject } from './settings'

export function createTestCommandClient(options: TestCommandClientOptions = {}): CommandClient {
  const state = createTestCommandClientState(options)

  return {
    ...createBaseCommandHandlers(state),
    ...createAgentCommandHandlers(state),
    ...createMcpCommandHandlers(state),
    ...createPluginCommandHandlers(state),
    ...createSettingsCommandHandlers(state),
    ...createSkillCommandHandlers(state),
  } satisfies CommandClient
}

export { createRejectedTestCommandClient } from './rejected'
