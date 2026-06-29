import { relaunch } from '@tauri-apps/plugin-process'
import { check, type DownloadEvent } from '@tauri-apps/plugin-updater'

export interface UpdateHandle {
  body?: string
  currentVersion: string
  date?: string
  downloadAndInstall: (onEvent?: (event: DownloadEvent) => void) => Promise<void>
  version: string
}

export interface AppUpdate {
  body?: string
  currentVersion: string
  date?: string
  handle: UpdateHandle
  version: string
}

export interface UpdaterClient {
  check: () => Promise<UpdateHandle | null>
  relaunch: () => Promise<void>
}

export type AppUpdateCheckResult =
  | {
      kind: 'current'
    }
  | {
      kind: 'available'
      update: AppUpdate
    }

export type UpdateDownloadProgress =
  | {
      contentLength?: number
      downloadedBytes: number
      kind: 'started'
    }
  | {
      contentLength?: number
      downloadedBytes: number
      kind: 'progress'
    }
  | {
      contentLength?: number
      downloadedBytes: number
      kind: 'finished'
    }

const tauriUpdaterClient: UpdaterClient = {
  check,
  relaunch,
}

export async function checkForAppUpdate(
  client: UpdaterClient = tauriUpdaterClient,
): Promise<AppUpdateCheckResult> {
  const update = await client.check()

  if (!update) {
    return { kind: 'current' }
  }

  return {
    kind: 'available',
    update: {
      body: update.body,
      currentVersion: update.currentVersion,
      date: update.date,
      handle: update,
      version: update.version,
    },
  }
}

export async function downloadAndInstallUpdate(
  update: AppUpdate,
  onProgress?: (event: UpdateDownloadProgress) => void,
) {
  let contentLength: number | undefined
  let downloadedBytes = 0

  await update.handle.downloadAndInstall((event) => {
    if (event.event === 'Started') {
      contentLength = event.data.contentLength
      downloadedBytes = 0
      onProgress?.({ contentLength, downloadedBytes, kind: 'started' })
      return
    }

    if (event.event === 'Progress') {
      downloadedBytes += event.data.chunkLength
      onProgress?.({ contentLength, downloadedBytes, kind: 'progress' })
      return
    }

    onProgress?.({ contentLength, downloadedBytes, kind: 'finished' })
  })
}

export function relaunchApp(client: UpdaterClient = tauriUpdaterClient) {
  return client.relaunch()
}
