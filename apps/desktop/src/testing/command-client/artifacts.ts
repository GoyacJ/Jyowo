import type {
  CreateAttachmentFromPathResponse,
  ExportSupportBundleResponse,
  GetArtifactMediaPreviewResponse,
  GetContextSnapshotResponse,
  ListArtifactsResponse,
  ListReferenceCandidatesResponse,
  ReplayTimelineResponse,
} from '@/shared/tauri/commands'

import { timestamp } from './base'
import { fixtureListActivity } from './conversation'

export const fixtureListArtifacts: ListArtifactsResponse = {
  artifacts: [
    {
      actionLabel: 'Run app',
      description: 'Tauri + React + TypeScript with Vite',
      id: 'artifact-desktop-foundation',
      kind: 'app',
      preview: 'Tauri command boundary, React renderer shell, and Vite development scripts.',
      status: 'ready',
      title: 'Desktop foundation created',
    },
    {
      actionLabel: 'Inspect',
      description: 'Follow-up verification checklist',
      id: 'artifact-verification-notes',
      kind: 'markdown',
      status: 'pending',
      title: 'Verification notes',
    },
    {
      actionLabel: 'Inspect diff',
      description: 'Runtime shell entrypoint changes',
      id: 'artifact-shell-diff',
      kind: 'diff',
      preview: [
        '--- src/main/main.ts',
        '+++ src/main/main.ts',
        "+ import { app, BrowserWindow, ipcMain } from 'electron'",
        "+ import path from 'path'",
        '+',
        '+ function createWindow () {',
        '+   const win = new BrowserWindow({',
        '+     width: 1200,',
        '+     height: 800,',
        '+     webPreferences: {',
        "+       preload: path.join(__dirname, 'preload.js'),",
        '+       contextIsolation: true,',
        '+     }',
        '+   })',
        "+   win.loadURL(process.env.VITE_DEV_SERVER_URL || 'index.html')",
        '+ }',
      ].join('\n'),
      status: 'ready',
      title: 'src/main/main.ts',
    },
  ],
}

export const fixtureAttachment: CreateAttachmentFromPathResponse = {
  attachment: {
    blobRef: {
      contentHash: Array.from({ length: 32 }, () => 1),
      contentType: 'text/plain',
      id: '01J00000000000000000000000',
      size: 128,
    },
    id: 'attachment-0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef',
    mimeType: 'text/plain',
    name: 'notes.txt',
    sizeBytes: 128,
  },
}

export const fixtureArtifactMediaPreview: GetArtifactMediaPreviewResponse = {
  dataUrl:
    'data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+/p9sAAAAASUVORK5CYII=',
  mimeType: 'image/png',
  sizeBytes: 68,
}

export const fixtureReferenceCandidates: ListReferenceCandidatesResponse = {
  artifacts: [{ id: 'artifact-desktop-foundation', label: 'Desktop foundation created' }],
  conversations: [{ id: 'conversation-001', label: 'Build the desktop foundation' }],
  files: [
    {
      label: 'apps/desktop/src/shared/tauri/commands.ts',
      path: 'apps/desktop/src/shared/tauri/commands.ts',
    },
  ],
  memories: [{ id: '01HZ0000000000000000000001', label: 'Prefers concise Chinese responses' }],
  mcpServers: [{ id: 'stdio', label: 'stdio' }],
  skills: [{ id: 'release-notes', label: 'release-notes' }],
  tools: [{ id: 'list_dir', label: 'List directory' }],
}

export const fixtureContextSnapshot: GetContextSnapshotResponse = {
  activeArtifact: 'App shell (WIP)',
  decisions: [{ detail: 'When: Before adding AI features', title: 'Choose IPC pattern' }],
  files: [
    { label: 'src/' },
    { label: 'public/' },
    { label: 'package.json' },
    { label: 'main.ts' },
    { label: 'preload.ts' },
    { label: 'vite.config.ts' },
  ],
  nextActions: ['Review changes', 'Continue', 'Open artifact'],
  path: '~/projects/desktop-app',
  project: 'Desktop App',
}

export const fixtureReplayTimeline: ReplayTimelineResponse = {
  events: fixtureListActivity.events,
  replayed: true,
}

export const fixtureSupportBundleExport: ExportSupportBundleResponse = {
  bundlePath: '.jyowo/runtime/exports/support-bundle-20260617T000000.000Z.json',
  eventCount: 1,
  exportedAt: timestamp,
  jsonlPath: '.jyowo/runtime/exports/events-20260617T000000.000Z.jsonl',
  markdownPath: '.jyowo/runtime/exports/support-report-20260617T000000.000Z.md',
  redacted: true,
}
