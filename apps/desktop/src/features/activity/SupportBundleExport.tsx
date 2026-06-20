import { useState } from 'react'
import { useTranslation } from 'react-i18next'

import type { ExportSupportBundleResponse } from '@/shared/tauri/commands'
import { Button } from '@/shared/ui/button'

type SupportBundleExportProps = {
  onExport: () => Promise<
    | ExportSupportBundleResponse
    | (Omit<ExportSupportBundleResponse, 'redacted'> & { redacted: boolean })
  >
}

type ExportState =
  | { status: 'idle' }
  | { status: 'exporting' }
  | { messageKey: 'failed' | 'notRedacted'; status: 'error' }
  | { result: ExportSupportBundleResponse; status: 'ready' }

export function SupportBundleExport({ onExport }: SupportBundleExportProps) {
  const { t } = useTranslation('activity')
  const [exportState, setExportState] = useState<ExportState>({
    status: 'idle',
  })

  async function handleExport() {
    setExportState({ status: 'exporting' })

    try {
      const result = await onExport()

      if (!result.redacted) {
        setExportState({
          messageKey: 'notRedacted',
          status: 'error',
        })
        return
      }

      setExportState({ result: { ...result, redacted: true }, status: 'ready' })
    } catch {
      setExportState({
        messageKey: 'failed',
        status: 'error',
      })
    }
  }

  return (
    <section aria-label={t('supportBundle.title')} className="space-y-4">
      <Button disabled={exportState.status === 'exporting'} onClick={handleExport} type="button">
        {exportState.status === 'exporting'
          ? t('supportBundle.exporting')
          : t('supportBundle.export')}
      </Button>

      {exportState.status === 'error' ? (
        <p className="text-destructive text-sm">{t(`supportBundle.${exportState.messageKey}`)}</p>
      ) : null}

      {exportState.status === 'ready' ? (
        <div className="space-y-2 text-sm">
          <p className="text-success">{t('supportBundle.redacted')}</p>
          <p>{t('supportBundle.events', { count: exportState.result.eventCount })}</p>
          <dl className="grid gap-2">
            <div>
              <dt className="text-muted-foreground text-xs">{t('supportBundle.bundle')}</dt>
              <dd className="font-mono text-xs">{exportState.result.bundlePath}</dd>
            </div>
            <div>
              <dt className="text-muted-foreground text-xs">{t('supportBundle.jsonl')}</dt>
              <dd className="font-mono text-xs">{exportState.result.jsonlPath}</dd>
            </div>
            <div>
              <dt className="text-muted-foreground text-xs">{t('supportBundle.markdown')}</dt>
              <dd className="font-mono text-xs">{exportState.result.markdownPath}</dd>
            </div>
          </dl>
        </div>
      ) : null}
    </section>
  )
}
