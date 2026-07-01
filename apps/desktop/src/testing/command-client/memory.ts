import type {
  ExportMemoryItemsResponse,
  GetMemoryItemResponse,
  ListMemoryItemsResponse,
} from '@/shared/tauri/commands'

import { timestamp } from './base'

export const fixtureMemoryItems: ListMemoryItemsResponse = {
  items: [
    {
      contentPreview: 'Prefers concise Chinese responses',
      id: '01HZ0000000000000000000001',
      kind: 'user_preference',
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
    createdAt: timestamp,
    id: '01HZ0000000000000000000001',
    kind: 'user_preference',
    source: 'user_input',
    tags: ['tone'],
    updatedAt: timestamp,
    visibility: 'tenant',
  },
}

export const fixtureMemoryExport: ExportMemoryItemsResponse = {
  exportedAt: timestamp,
  format: 'json',
  itemCount: 1,
  path: '.jyowo/runtime/exports/memory-20260617T000000.000Z.json',
}
