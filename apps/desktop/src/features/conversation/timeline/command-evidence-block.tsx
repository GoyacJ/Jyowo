import { Copy } from 'lucide-react'
import { useTranslation } from 'react-i18next'
import { cn } from '@/shared/lib/utils'
import type { CommandExecution } from '@/shared/tauri/commands'

export function CommandEvidenceBlock({ execution }: { execution: CommandExecution }) {
  const { t } = useTranslation('conversation')
  const output =
    execution.redactionState === 'withheld' ? null : (execution.stdoutPreview ?? null)

  const handleCopy = async () => {
    const lines = [`$ ${execution.command}`]
    if (output) lines.push('', output)
    if (execution.exitCode !== undefined) lines.push('', `exit ${execution.exitCode}`)
    if (execution.durationMs !== undefined) lines.push(`duration ${execution.durationMs} ms`)
    await navigator.clipboard?.writeText(lines.join('\n'))
  }

  return (
    <section className="overflow-hidden rounded-md border border-border bg-terminal-background text-white">
      <div className="flex h-8 items-center justify-between gap-3 border-white/10 border-b px-3">
        <div className="flex items-center gap-2">
          <span className="font-medium text-xs">{t('timeline.commandEvidence.shell')}</span>
          {execution.redactionState !== 'clean' ? (
            <span className="rounded bg-yellow-500/20 px-1 py-0 text-yellow-400 text-xs">
              {execution.redactionState}
            </span>
          ) : null}
        </div>
        <button
          aria-label={t('timeline.commandEvidence.copy')}
          className="inline-flex size-7 items-center justify-center rounded-md text-white/60 hover:bg-white/10 hover:text-white focus-visible:ring-2 focus-visible:ring-ring"
          onClick={handleCopy}
          type="button"
        >
          <Copy className="size-3.5" />
        </button>
      </div>
      <div className="border-white/10 border-b px-3 py-2 font-mono text-xs text-white/90">
        $ {execution.command}
      </div>
      {output ? (
        <pre className="max-h-[260px] overflow-auto px-3 py-2 font-mono text-xs leading-5">
          <code>{output}</code>
        </pre>
      ) : execution.redactionState === 'withheld' ? (
        <div className="px-3 py-4 text-center text-muted-foreground text-xs">
          {t('timeline.commandEvidence.withheld')}
        </div>
      ) : null}
      <div className="flex min-h-7 items-center justify-end gap-3 border-white/10 border-t px-3 text-xs">
        {execution.durationMs !== undefined ? (
          <span className="text-white/55">
            {t('timeline.commandEvidence.duration', { duration: execution.durationMs })}
          </span>
        ) : null}
        {execution.exitCode !== undefined ? (
          <span className={cn(execution.exitCode === 0 ? 'text-white/65' : 'text-destructive')}>
            {t('timeline.commandEvidence.exitCode', { code: execution.exitCode })}
          </span>
        ) : null}
        {execution.truncated ? (
          <span className="text-yellow-400/80">{t('timeline.commandEvidence.truncated')}</span>
        ) : null}
      </div>
    </section>
  )
}
