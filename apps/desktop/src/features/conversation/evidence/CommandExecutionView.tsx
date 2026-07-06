import { Copy } from 'lucide-react'
import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import { cn } from '@/shared/lib/utils'
import type { CommandExecution } from '@/shared/tauri/commands'
import { useCommandClient } from '@/shared/tauri/react'

export function CommandExecutionView({
  allowFullOutputFetch = true,
  command,
  conversationId,
  density = 'inspector',
}: {
  allowFullOutputFetch?: boolean
  command: CommandExecution
  conversationId: string
  density?: 'timeline' | 'inspector'
}) {
  const { t } = useTranslation('conversation')
  const [fetchedOutput, setFetchedOutput] = useState<{
    hasMore: boolean
    nextCursor?: string
    output: string
    truncated: boolean
  } | null>(null)
  const [copyFailed, setCopyFailed] = useState(false)
  const [fetchFailed, setFetchFailed] = useState(false)

  const handleCopyCommand = async () => {
    try {
      setCopyFailed(false)
      if (!navigator.clipboard) throw new Error('Clipboard unavailable')
      await navigator.clipboard.writeText(command.command)
    } catch {
      setCopyFailed(true)
    }
  }

  const previewOutput = [command.stdoutPreview, command.stderrPreview].filter(Boolean).join('\n')
  const visibleOutput = fetchedOutput?.output ?? (previewOutput.length > 0 ? previewOutput : null)
  const canFetchFullOutput =
    allowFullOutputFetch && command.fullOutputRef && command.redactionState !== 'withheld'

  const handleCopyVisible = async () => {
    if (!visibleOutput) return
    try {
      setCopyFailed(false)
      if (!navigator.clipboard) throw new Error('Clipboard unavailable')
      await navigator.clipboard.writeText(visibleOutput)
    } catch {
      setCopyFailed(true)
    }
  }

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
          <CopyButton
            label={t('timeline.commandEvidence.copyCommand')}
            onClick={() => void handleCopyCommand()}
          />
          {visibleOutput ? (
            <CopyButton
              label={t('timeline.commandEvidence.copyOutput')}
              onClick={() => void handleCopyVisible()}
            />
          ) : null}
          {canFetchFullOutput ? (
            <FullOutputFetchButton
              command={command}
              conversationId={conversationId}
              loaded={fetchedOutput !== null}
              onError={() => {
                setFetchedOutput(null)
                setFetchFailed(true)
              }}
              onLoad={(response) => {
                setFetchFailed(false)
                setFetchedOutput(response)
              }}
              onStart={() => setFetchFailed(false)}
            />
          ) : null}
        </div>
      </div>
      {copyFailed ? (
        <div className="border-white/10 border-b px-3 py-1 text-red-300 text-xs">
          {t('timeline.commandEvidence.copyFailed', 'Copy failed')}
        </div>
      ) : null}
      {fetchFailed ? (
        <div className="border-white/10 border-b px-3 py-1 text-red-300 text-xs">
          {t('timeline.commandEvidence.fetchFailed', 'Failed to load output page')}
        </div>
      ) : null}

      {/* Command line */}
      <div className="border-white/10 border-b px-3 py-2 font-mono text-xs text-white/90">
        $ {command.command}
      </div>

      {/* Metadata bar */}
      <div className="flex min-h-7 flex-wrap items-center gap-x-3 gap-y-1 border-white/10 border-b px-3 py-1 text-xs">
        {command.cwd ? <span className="text-white/55">{command.cwd}</span> : null}
        {command.shell ? <span className="text-white/55">shell: {command.shell}</span> : null}
        {command.durationMs ? (
          <span className="text-white/55">
            {t('timeline.commandEvidence.duration', { duration: command.durationMs })}
          </span>
        ) : null}
        {command.exitCode !== undefined ? (
          <span className={cn(command.exitCode === 0 ? 'text-white/65' : 'text-red-400')}>
            {t('timeline.commandEvidence.exitCode', { code: command.exitCode })}
          </span>
        ) : null}
        {command.truncated ? (
          <span className="text-yellow-400/80">{t('timeline.commandEvidence.truncated')}</span>
        ) : null}
        {fetchedOutput?.truncated ? (
          <span className="text-yellow-400/80">
            {t('timeline.commandEvidence.pageTruncated', 'Output page truncated')}
          </span>
        ) : null}
      </div>

      {/* Output */}
      {visibleOutput ? (
        <pre
          className={cn(
            'overflow-auto px-3 py-2 font-mono text-xs leading-5',
            density === 'timeline' ? 'max-h-[260px]' : 'max-h-[360px]',
          )}
          data-testid="command-output-scroll-region"
        >
          <code>{visibleOutput}</code>
        </pre>
      ) : command.redactionState === 'withheld' ? (
        <div className="px-3 py-4 text-center text-muted-foreground text-xs">
          {t('timeline.commandEvidence.withheld')}
        </div>
      ) : null}
    </section>
  )
}

function FullOutputFetchButton({
  command,
  conversationId,
  loaded,
  onError,
  onLoad,
  onStart,
}: {
  command: CommandExecution
  conversationId: string
  loaded: boolean
  onError: () => void
  onLoad: (response: {
    hasMore: boolean
    nextCursor?: string
    output: string
    truncated: boolean
  }) => void
  onStart: () => void
}) {
  const { t } = useTranslation('conversation')
  const commandClient = useCommandClient()
  const [fetchingPage, setFetchingPage] = useState(false)

  const handleFetchPage = async () => {
    if (!command.fullOutputRef) return
    setFetchingPage(true)
    onStart()
    try {
      const response = await commandClient.getConversationCommandOutput({
        conversationId,
        fullOutputRef: command.fullOutputRef,
      })
      onLoad({
        hasMore: response.hasMore,
        nextCursor: response.nextCursor,
        output: response.output,
        truncated: response.truncated,
      })
    } catch {
      onError()
    } finally {
      setFetchingPage(false)
    }
  }

  return (
    <button
      className="rounded px-2 py-0.5 text-white/60 text-xs hover:bg-white/10 hover:text-white focus-visible:ring-2 focus-visible:ring-ring"
      disabled={fetchingPage}
      onClick={handleFetchPage}
      type="button"
    >
      {fetchingPage
        ? t('timeline.commandEvidence.fetching')
        : loaded
          ? t('timeline.commandEvidence.pageLoaded')
          : t('timeline.commandEvidence.fetchPage')}
    </button>
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
