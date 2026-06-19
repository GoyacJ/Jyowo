import { useState } from 'react'

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
  | { message: string; status: 'error' }
  | { result: ExportSupportBundleResponse; status: 'ready' }

export function SupportBundleExport({ onExport }: SupportBundleExportProps) {
  const [exportState, setExportState] = useState<ExportState>({
    status: 'idle',
  })

  async function handleExport() {
    setExportState({ status: 'exporting' })

    try {
      const result = await onExport()

      if (!result.redacted) {
        setExportState({
          message: 'Support bundle export was not redacted.',
          status: 'error',
        })
        return
      }

      setExportState({ result: { ...result, redacted: true }, status: 'ready' })
    } catch {
      setExportState({
        message: 'Support bundle export failed.',
        status: 'error',
      })
    }
  }

  return (
    <section aria-label="Support bundle export" className="space-y-4">
      <Button disabled={exportState.status === 'exporting'} onClick={handleExport} type="button">
        {exportState.status === 'exporting' ? 'Exporting support bundle' : 'Export support bundle'}
      </Button>

      {exportState.status === 'error' ? (
        <p className="text-destructive text-sm">{exportState.message}</p>
      ) : null}

      {exportState.status === 'ready' ? (
        <div className="space-y-2 text-sm">
          <p className="text-success">Redacted</p>
          <p>{exportState.result.eventCount} events</p>
          <dl className="grid gap-2">
            <div>
              <dt className="text-muted-foreground text-xs">Bundle</dt>
              <dd className="font-mono text-xs">{exportState.result.bundlePath}</dd>
            </div>
            <div>
              <dt className="text-muted-foreground text-xs">JSONL</dt>
              <dd className="font-mono text-xs">{exportState.result.jsonlPath}</dd>
            </div>
            <div>
              <dt className="text-muted-foreground text-xs">Markdown</dt>
              <dd className="font-mono text-xs">{exportState.result.markdownPath}</dd>
            </div>
          </dl>
        </div>
      ) : null}
    </section>
  )
}
