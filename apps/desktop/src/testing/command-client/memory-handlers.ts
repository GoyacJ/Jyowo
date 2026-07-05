import {
  DEFAULT_MEMORY_TENANT_ID,
  type GetMemorySettingsResponse,
  type UpdateMemoryItemResponse,
} from '@/shared/tauri/commands'

import { wait } from './base'
import { fixtureMemoryExport, fixtureMemoryItem, fixtureMemoryItems } from './memory'
import type { TestCommandClientState, TestCommandHandlers } from './state'

type MemoryCommandKeys =
  | 'approveMemoryCandidate'
  | 'deleteMemoryItem'
  | 'exportMemoryItems'
  | 'getMemoryItem'
  | 'getMemorySettings'
  | 'getMemoryRecallTrace'
  | 'getModelRequestPreview'
  | 'getThreadMemorySettings'
  | 'listMemoryCandidates'
  | 'listMemoryItems'
  | 'listMemoryRecallTraces'
  | 'mergeMemoryCandidate'
  | 'rejectMemoryCandidate'
  | 'updateMemoryItem'
  | 'updateMemorySettings'
  | 'updateThreadMemorySettings'

export function createMemoryCommandHandlers(
  state: TestCommandClientState,
): TestCommandHandlers<MemoryCommandKeys> {
  return {
    async approveMemoryCandidate(request) {
      await wait(state.options.delayMs)
      return {
        candidate: {
          created_at: '2026-06-17T00:00:00.000Z',
          evidence: defaultMemoryEvidence(),
          expires_at: null,
          id: request.candidateId,
          operation: 'create',
          proposed_record: defaultMemoryDraft(),
          state: 'promoted',
          tenant_id: request.tenantId ?? DEFAULT_MEMORY_TENANT_ID,
          updated_at: '2026-06-17T00:00:00.000Z',
        },
        memory_id: '01HZ0000000000000000000001',
      }
    },
    async deleteMemoryItem(request) {
      await wait(state.options.delayMs)
      return { id: request.id, status: 'deleted' }
    },
    async exportMemoryItems(_request) {
      await wait(state.options.delayMs)
      return state.options.memoryExport ?? fixtureMemoryExport
    },
    async getMemoryItem() {
      await wait(state.options.delayMs)
      return state.options.memoryItem ?? fixtureMemoryItem
    },
    async getMemorySettings() {
      await wait(state.options.delayMs)
      return defaultMemorySettingsResponse()
    },
    async getMemoryRecallTrace(request) {
      await wait(state.options.delayMs)
      return {
        trace: {
          at: '2026-06-17T00:00:00.000Z',
          candidates: [],
          deadline_used_ms: 250,
          dropped: [],
          injected: [],
          injected_chars: 0,
          provider_results: [],
          query_text_hash: defaultContentHash(),
          redacted_count: 0,
          run_id: '01HZ0000000000000000000003',
          session_id: '01HZ0000000000000000000004',
          tenant_id: request.tenantId ?? DEFAULT_MEMORY_TENANT_ID,
          trace_id: request.traceId,
          turn: 0,
        },
      }
    },
    async getModelRequestPreview(request) {
      await wait(state.options.delayMs)
      return {
        preview: {
          content_hash: defaultContentHash(),
          policy_decisions: [],
          redacted_count: 0,
          run_id: request.runId,
          sections: [],
          session_id: request.sessionId,
          token_estimate: 0,
          tool_names: [],
          trace_id: request.traceId,
        },
      }
    },
    async getThreadMemorySettings(request) {
      await wait(state.options.delayMs)
      return {
        settings: {
          generate_memories: null,
          memory_mode: 'read_write',
          session_id: request.sessionId,
          use_memories: null,
        },
      }
    },
    async listMemoryCandidates() {
      await wait(state.options.delayMs)
      return { candidates: [], next_cursor: null }
    },
    async listMemoryItems() {
      await wait(state.options.delayMs)
      return state.options.memoryItems ?? fixtureMemoryItems
    },
    async listMemoryRecallTraces() {
      await wait(state.options.delayMs)
      return { next_cursor: null, traces: [] }
    },
    async mergeMemoryCandidate(request) {
      await wait(state.options.delayMs)
      return {
        candidate_ids: request.candidateIds,
        memory_id: '01HZ0000000000000000000001',
      }
    },
    async rejectMemoryCandidate(request) {
      await wait(state.options.delayMs)
      return {
        candidate: {
          created_at: '2026-06-17T00:00:00.000Z',
          evidence: defaultMemoryEvidence(),
          expires_at: null,
          id: request.candidateId,
          operation: 'create',
          proposed_record: defaultMemoryDraft(),
          state: 'rejected',
          tenant_id: request.tenantId ?? DEFAULT_MEMORY_TENANT_ID,
          updated_at: '2026-06-17T00:00:00.000Z',
        },
      }
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
    async updateMemorySettings(request) {
      await wait(state.options.delayMs)
      return { settings: request.settings }
    },
    async updateThreadMemorySettings(request) {
      await wait(state.options.delayMs)
      return { settings: request.settings }
    },
  }
}

function defaultMemorySettingsResponse(): GetMemorySettingsResponse {
  return {
    settings: {
      disable_generation_when_external_context_used: false,
      generate_memories: true,
      max_memory_bytes: 1_000_000,
      max_recall_chars_per_turn: 4_000,
      max_recall_records_per_turn: 5,
      retention_days: null,
      use_memories: true,
    },
  }
}

function defaultMemoryDraft() {
  return {
    content: 'Candidate memory entry',
    expires_at: null,
    kind: 'user_preference' as const,
    metadata: {
      source_trust: 0.8,
      tags: ['tone'],
      ttl: null,
    },
    visibility: 'tenant' as const,
  }
}

function defaultMemoryEvidence() {
  return {
    content_hash: defaultContentHash(),
    origin: {
      imported: {
        import_id: '01HZ0000000000000000000002',
        importer: 'test',
      },
    },
    source: 'user_input' as const,
  }
}

function defaultContentHash() {
  return Array.from({ length: 32 }, () => 1)
}
