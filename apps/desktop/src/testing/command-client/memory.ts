import type {
  ExportMemoryItemsResponse,
  GetMemoryItemResponse,
  ListMemoryItemsResponse,
} from '@/shared/tauri/commands'

import { timestamp } from './base'

export const fixtureMemoryItems: ListMemoryItemsResponse = {
  items: [
    {
      contentHash: '0'.repeat(64),
      contentPreview: 'Prefers concise Chinese responses',
      deleted: false,
      id: '01HZ0000000000000000000001',
      kind: 'user_preference',
      lastAccessedAt: null,
      providerId: 'local',
      source: 'user_input',
      tags: ['tone'],
      updatedAt: timestamp,
      visibility: 'tenant',
    },
  ],
}

export const fixtureMemoryItem: GetMemoryItemResponse = {
  item: {
    accessCount: 0,
    confidence: 1,
    content: 'Prefers concise Chinese responses',
    contentHash: '0'.repeat(64),
    createdAt: timestamp,
    deleted: false,
    id: '01HZ0000000000000000000001',
    kind: 'user_preference',
    lastAccessedAt: null,
    providerId: 'local',
    source: 'user_input',
    tags: ['tone'],
    updatedAt: timestamp,
    visibility: 'tenant',
  },
}

export const fixtureMemoryExport: ExportMemoryItemsResponse = {
  auditHash: '0'.repeat(64),
  exportedAt: timestamp,
  format: 'json',
  includeHashes: true,
  includeMetadata: true,
  includeRawContent: false,
  itemCount: 1,
  path: '.jyowo/runtime/exports/memory-20260617T000000.000Z.json',
  scope: 'visible',
}
