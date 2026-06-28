import { Copy } from 'lucide-react'
import { useTranslation } from 'react-i18next'
import { cn } from '@/shared/lib/utils'
import { Tooltip, TooltipContent, TooltipProvider, TooltipTrigger } from '@/shared/ui/tooltip'

export type CommandEvidenceBlockProps = {
  command: string
  durationMs?: number
  exitCode?: number
  output?: string
}

export function CommandEvidenceBlock({
  command,
  durationMs,
  exitCode,
  output,
}: CommandEvidenceBlockProps) {
  const { t } = useTranslation('conversation')

  return (
    <section className="overflow-hidden rounded-md border border-border bg-terminal-background text-white">
      <div className="flex h-8 items-center justify-between gap-3 border-white/10 border-b px-3">
        <div className="font-medium text-xs">{t('timeline.commandEvidence.shell')}</div>
        <TooltipProvider delayDuration={150}>
          <Tooltip>
            <TooltipTrigger asChild>
              <button
                aria-label={t('timeline.commandEvidence.copy')}
                className="inline-flex size-7 items-center justify-center rounded-md text-white/60 hover:bg-white/10 hover:text-white focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring"
                type="button"
                onClick={() => {
                  void copyCommandEvidence({ command, durationMs, exitCode, output })
                }}
              >
                <Copy className="size-3.5" />
              </button>
            </TooltipTrigger>
            <TooltipContent>{t('timeline.commandEvidence.copy')}</TooltipContent>
          </Tooltip>
        </TooltipProvider>
      </div>
      <div className="border-white/10 border-b px-3 py-2 font-mono text-xs text-white/90">
        $ {command}
      </div>
      {output ? (
        <pre
          className="max-h-[260px] overflow-auto px-3 py-2 font-mono text-xs leading-5"
          data-testid="command-output-scroll-region"
        >
          <code>{output}</code>
        </pre>
      ) : null}
      {exitCode !== undefined || durationMs !== undefined ? (
        <div className="flex min-h-7 items-center justify-end gap-3 border-white/10 border-t px-3 text-xs">
          {durationMs !== undefined ? (
            <span className="text-white/55">
              {t('timeline.commandEvidence.duration', { duration: durationMs })}
            </span>
          ) : null}
          {exitCode !== undefined ? (
            <span className={cn(exitCode === 0 ? 'text-white/65' : 'text-destructive')}>
              {t('timeline.commandEvidence.exitCode', { code: exitCode })}
            </span>
          ) : null}
        </div>
      ) : null}
    </section>
  )
}

async function copyCommandEvidence({
  command,
  durationMs,
  exitCode,
  output,
}: CommandEvidenceBlockProps) {
  const lines = [`$ ${command}`]
  if (output) {
    lines.push('', output)
  }
  if (exitCode !== undefined) {
    lines.push('', `exit ${exitCode}`)
  }
  if (durationMs !== undefined) {
    lines.push(`duration ${durationMs} ms`)
  }

  await navigator.clipboard?.writeText(lines.join('\n'))
}
