import type { UpdateMemoryItemResponse } from '@/shared/tauri/commands'

import { wait } from './base'
import { fixtureMemoryExport, fixtureMemoryItem, fixtureMemoryItems } from './memory'
import type { TestCommandClientState, TestCommandHandlers } from './state'

type MemoryCommandKeys =
  | 'deleteMemoryItem'
  | 'exportMemoryItems'
  | 'getMemoryItem'
  | 'listMemoryItems'
  | 'updateMemoryItem'

export function createMemoryCommandHandlers(
  state: TestCommandClientState,
): TestCommandHandlers<MemoryCommandKeys> {
  return {
    async deleteMemoryItem(id) {
      await wait(state.options.delayMs)
      return { id, status: 'deleted' }
    },
    async exportMemoryItems() {
      await wait(state.options.delayMs)
      return state.options.memoryExport ?? fixtureMemoryExport
    },
    async getMemoryItem() {
      await wait(state.options.delayMs)
      return state.options.memoryItem ?? fixtureMemoryItem
    },
    async listMemoryItems() {
      await wait(state.options.delayMs)
      return state.options.memoryItems ?? fixtureMemoryItems
    },
    async updateMemoryItem(request) {
      await wait(state.options.delayMs)
      return {
        item: {
          ...(state.options.memoryItem ?? fixtureMemoryItem).item,
          content: request.content,
          id: request.id,
        },
      } satisfies UpdateMemoryItemResponse
    },
  }
}
