import { Copy } from 'lucide-react'
import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import { cn } from '@/shared/lib/utils'
import { useCommandClient } from '@/shared/tauri/react'
import type { CommandExecution } from '@/shared/tauri/commands'

export function CommandExecutionView({
  command,
  conversationId,
  onOpenInspector,
}: {
  command: CommandExecution
  conversationId: string
  onOpenInspector?: () => void
}) {
  const { t } = useTranslation('conversation')
  const commandClient = useCommandClient()
  const [fetchingFull, setFetchingFull] = useState(false)
  const [fullOutput, setFullOutput] = useState<string | null>(null)
  const canFetchFull =
    Boolean(command.fullOutputRef) && command.redactionState !== 'withheld'

  const handleCopyCommand = () => {
    void navigator.clipboard?.writeText(command.command)
  }

  const handleCopyVisible = () => {
    const parts = [`$ ${command.command}`]
    if (command.stdoutPreview) parts.push('', command.stdoutPreview)
    if (command.stderrPreview) parts.push('', command.stderrPreview)
    void navigator.clipboard?.writeText(parts.join('\n'))
  }

  const handleFetchFull = async () => {
    if (!command.fullOutputRef) return
    setFetchingFull(true)
    try {
      const response = await commandClient.getConversationCommandOutput({
        conversationId,
        fullOutputRef: command.fullOutputRef,
      })
      setFullOutput(response.output)
    } catch {
      setFullOutput(null)
    } finally {
      setFetchingFull(false)
    }
  }

  const output = fullOutput ?? command.stdoutPreview ?? null

  return (
    <section className="overflow-hidden rounded-md border border-border bg-terminal-background text-white">
      {/* Header */}
      <div className="flex h-8 items-center justify-between gap-3 border-white/10 border-b px-3">
        <div className="flex items-center gap-2">
          <span className="font-medium text-xs">{t('timeline.commandEvidence.shell')}</span>
          {command.sandbox ? (
            <span className="rounded bg-white/10 px-1 py-0 text-muted-foreground text-xs">
              {command.sandbox}
            </span>
          ) : null}
          {command.redactionState !== 'clean' ? (
            <span className="rounded bg-yellow-500/20 px-1 py-0 text-yellow-400 text-xs">
              {command.redactionState}
            </span>
          ) : null}
        </div>
        <div className="flex gap-1">
          <CopyButton label={t('timeline.commandEvidence.copyCommand')} onClick={handleCopyCommand} />
          {output ? (
            <CopyButton
              label={t('timeline.commandEvidence.copyOutput')}
              onClick={handleCopyVisible}
            />
          ) : null}
          {command.fullOutputRef ? (
            <button
              className="rounded px-2 py-0.5 text-white/60 text-xs hover:bg-white/10 hover:text-white focus-visible:ring-2 focus-visible:ring-ring"
              disabled={fetchingFull}
              onClick={handleFetchFull}
              type="button"
            >
              {fetchingFull
                ? t('timeline.commandEvidence.fetching')
                : fullOutput
                  ? t('timeline.commandEvidence.fullFetched')
                  : t('timeline.commandEvidence.fetchFull')}
            </button>
          ) : null}
        </div>
      </div>

      {/* Command line */}
      <div className="border-white/10 border-b px-3 py-2 font-mono text-xs text-white/90">
        $ {command.command}
      </div>

      {/* Metadata bar */}
      <div className="flex min-h-7 flex-wrap items-center gap-x-3 gap-y-1 border-white/10 border-b px-3 py-1 text-xs">
        {command.cwd ? (
          <span className="text-white/55">{command.cwd}</span>
        ) : null}
        {command.shell ? (
          <span className="text-white/55">shell: {command.shell}</span>
        ) : null}
        {command.durationMs ? (
          <span className="text-white/55">
            {t('timeline.commandEvidence.duration', { duration: command.durationMs })}
          </span>
        ) : null}
        {command.exitCode !== undefined ? (
          <span className={cn(command.exitCode === 0 ? 'text-white/65' : 'text-red-400')}>
            exit {command.exitCode}
          </span>
        ) : null}
        {command.truncated ? (
          <span className="text-yellow-400/80">
            {t('timeline.commandEvidence.truncated')}
          </span>
        ) : null}
      </div>

      {/* Output */}
      {output ? (
        <pre
          className="max-h-[360px] overflow-auto px-3 py-2 font-mono text-xs leading-5"
          data-testid="command-output-scroll-region"
        >
          <code>{output}</code>
        </pre>
      ) : command.redactionState === 'withheld' ? (
        <div className="px-3 py-4 text-center text-muted-foreground text-xs">
          {t('timeline.commandEvidence.withheld')}
        </div>
      ) : null}
    </section>
  )
}

function CopyButton({ label, onClick }: { label: string; onClick: () => void }) {
  return (
    <button
      aria-label={label}
      className="inline-flex size-6 items-center justify-center rounded text-white/60 hover:bg-white/10 hover:text-white focus-visible:ring-2 focus-visible:ring-ring"
      onClick={onClick}
      type="button"
    >
      <Copy className="size-3" />
    </button>
  )
}
