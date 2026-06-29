import { useQuery } from '@tanstack/react-query'
import { Download, Info, RefreshCw } from 'lucide-react'
import { useState } from 'react'
import { useTranslation } from 'react-i18next'

import { getAppInfo } from '@/shared/tauri/commands'
import { useCommandClient } from '@/shared/tauri/react'
import {
  type AppUpdate,
  checkForAppUpdate,
  downloadAndInstallUpdate,
  relaunchApp,
} from '@/shared/tauri/updater'
import { Badge } from '@/shared/ui/badge'
import { Button } from '@/shared/ui/button'

type AboutUpdateState =
  | { kind: 'idle' }
  | { kind: 'checking' }
  | { kind: 'current' }
  | { kind: 'available'; update: AppUpdate }
  | {
      contentLength?: number
      downloadedBytes: number
      kind: 'downloading'
      update: AppUpdate
    }
  | { kind: 'installed'; update: AppUpdate }
  | { kind: 'failed'; message: string; update?: AppUpdate }

function getErrorMessage(error: unknown): string {
  return error instanceof Error ? error.message : String(error)
}

function updateForState(state: AboutUpdateState): AppUpdate | undefined {
  return 'update' in state ? state.update : undefined
}

function progressPercent(state: AboutUpdateState): number | undefined {
  if (state.kind !== 'downloading' || !state.contentLength || state.contentLength <= 0) {
    return undefined
  }

  return Math.min(100, Math.round((state.downloadedBytes / state.contentLength) * 100))
}

export function AboutSettings() {
  const { t } = useTranslation('settings')
  const commandClient = useCommandClient()
  const appInfoQuery = useQuery({
    queryKey: ['settings', 'about', 'app-info'],
    queryFn: () => getAppInfo(commandClient),
  })
  const [updateState, setUpdateState] = useState<AboutUpdateState>({ kind: 'idle' })
  const appInfo = appInfoQuery.data
  const update = updateForState(updateState)
  const percent = progressPercent(updateState)
  const releaseNotes = update?.body?.trim()
  const isChecking = updateState.kind === 'checking'
  const isDownloading = updateState.kind === 'downloading'

  async function handleCheckForUpdate() {
    setUpdateState({ kind: 'checking' })

    try {
      const result = await checkForAppUpdate()
      setUpdateState(result.kind === 'current' ? { kind: 'current' } : result)
    } catch (error) {
      setUpdateState({ kind: 'failed', message: getErrorMessage(error) })
    }
  }

  async function handleDownloadAndInstall() {
    if (!update) {
      return
    }

    setUpdateState({ downloadedBytes: 0, kind: 'downloading', update })

    try {
      await downloadAndInstallUpdate(update, (event) => {
        setUpdateState({
          contentLength: event.contentLength,
          downloadedBytes: event.downloadedBytes,
          kind: 'downloading',
          update,
        })
      })
      setUpdateState({ kind: 'installed', update })
      await relaunchApp()
    } catch (error) {
      setUpdateState({ kind: 'failed', message: getErrorMessage(error), update })
    }
  }

  return (
    <section className="space-y-5 rounded-md border border-border bg-surface p-5">
      <div className="flex flex-col gap-4 sm:flex-row sm:items-start sm:justify-between">
        <div className="flex items-start gap-3">
          <div className="rounded-md border border-border bg-background p-2 text-muted-foreground">
            <Info className="size-4" data-icon="inline-start" />
          </div>
          <div>
            <h2 className="font-semibold text-base">{t('about.title')}</h2>
            <p className="mt-1 text-muted-foreground text-sm">{t('about.description')}</p>
          </div>
        </div>
        <Button
          disabled={isChecking || isDownloading}
          onClick={() => {
            void handleCheckForUpdate()
          }}
          size="sm"
          variant="outline"
        >
          <RefreshCw data-icon="inline-start" />
          {isChecking ? t('about.actions.checking') : t('about.actions.check')}
        </Button>
      </div>

      <dl className="grid grid-cols-1 gap-3 sm:grid-cols-3">
        <div className="rounded-md border border-border bg-muted/35 p-4">
          <dt className="text-muted-foreground text-xs uppercase tracking-normal">
            {t('about.currentVersion')}
          </dt>
          <dd className="mt-2 overflow-wrap-anywhere font-medium">
            {appInfoQuery.isLoading ? t('about.loadingVersion') : (appInfo?.version ?? '-')}
          </dd>
        </div>
        <div className="rounded-md border border-border bg-muted/35 p-4">
          <dt className="text-muted-foreground text-xs uppercase tracking-normal">
            {t('about.updateStatus')}
          </dt>
          <dd className="mt-2">
            <Badge variant={updateState.kind === 'failed' ? 'destructive' : 'outline'}>
              {t(`about.status.${updateState.kind}`)}
            </Badge>
          </dd>
        </div>
        <div className="rounded-md border border-border bg-muted/35 p-4">
          <dt className="text-muted-foreground text-xs uppercase tracking-normal">
            {t('about.latestVersion')}
          </dt>
          <dd className="mt-2 overflow-wrap-anywhere font-medium">{update?.version ?? '-'}</dd>
        </div>
      </dl>

      {updateState.kind === 'failed' ? (
        <p className="rounded-md border border-destructive/30 bg-destructive/5 p-3 text-destructive text-sm">
          {updateState.message}
        </p>
      ) : null}

      {updateState.kind === 'available' ? (
        <Button
          disabled={isDownloading}
          onClick={() => {
            void handleDownloadAndInstall()
          }}
        >
          <Download data-icon="inline-start" />
          {t('about.actions.downloadInstall')}
        </Button>
      ) : null}

      {updateState.kind === 'downloading' ? (
        <div className="space-y-2">
          <div className="h-2 overflow-hidden rounded-full bg-muted">
            <div
              aria-label={t('about.downloadProgress')}
              aria-valuemax={100}
              aria-valuemin={0}
              aria-valuenow={percent ?? 0}
              className="h-full bg-primary transition-[width]"
              role="progressbar"
              style={{ width: `${percent ?? 0}%` }}
            />
          </div>
          <p className="text-muted-foreground text-sm">
            {percent === undefined
              ? t('about.downloadProgressUnknown')
              : t('about.downloadProgressPercent', { percent })}
          </p>
        </div>
      ) : null}

      <section className="space-y-2">
        <h3 className="font-medium text-sm">{t('about.releaseNotes')}</h3>
        {releaseNotes ? (
          <pre className="max-h-72 overflow-auto whitespace-pre-wrap rounded-md border border-border bg-background p-3 text-sm">
            {releaseNotes}
          </pre>
        ) : (
          <p className="text-muted-foreground text-sm">{t('about.releaseNotesEmpty')}</p>
        )}
      </section>
    </section>
  )
}
