import { Copy } from 'lucide-react'
import { useEffect, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { cn } from '@/shared/lib/utils'
import type { CommandExecution } from '@/shared/tauri/commands'
import { useCommandClient } from '@/shared/tauri/react'

export function CommandExecutionView({
  allowFullOutputFetch = false,
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
    refId: string
    truncated: boolean
  } | null>(null)
  const [copyFailed, setCopyFailed] = useState(false)
  const [fetchFailed, setFetchFailed] = useState(false)
  const currentOutputRef = useRef({
    fullOutputRef: command.fullOutputRef,
    redactionState: command.redactionState,
  })

  currentOutputRef.current = {
    fullOutputRef: command.fullOutputRef,
    redactionState: command.redactionState,
  }

  const isCurrentFetch = (requestedFullOutputRef: string) =>
    currentOutputRef.current.fullOutputRef === requestedFullOutputRef &&
    currentOutputRef.current.redactionState !== 'withheld'

  useEffect(() => {
    setFetchedOutput(null)
  }, [command.fullOutputRef, command.redactionState])

  const handleCopyCommand = async () => {
    try {
      setCopyFailed(false)
      if (!navigator.clipboard) throw new Error('Clipboard unavailable')
      await navigator.clipboard.writeText(command.command)
    } catch {
      setCopyFailed(true)
    }
  }

  const previewOutput =
    command.redactionState === 'withheld'
      ? ''
      : [command.stdoutPreview, command.stderrPreview].filter(Boolean).join('\n')
  const activeFetchedOutput =
    fetchedOutput?.refId === command.fullOutputRef && command.redactionState !== 'withheld'
      ? fetchedOutput
      : null
  const visibleOutput =
    command.redactionState === 'withheld'
      ? null
      : (activeFetchedOutput?.output ?? (previewOutput.length > 0 ? previewOutput : null))
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
    <section className="overflow-hidden rounded-md border border-border bg-terminal-background text-terminal-foreground">
      {/* Header */}
      <div className="flex h-8 items-center justify-between gap-3 border-terminal-border border-b px-3">
        <div className="flex items-center gap-2">
          <span className="font-medium text-xs">{t('timeline.commandEvidence.shell')}</span>
          {command.sandbox ? (
            <span className="rounded bg-terminal-control px-1 py-0 text-terminal-muted text-xs">
              {command.sandbox}
            </span>
          ) : null}
          {command.redactionState !== 'clean' ? (
            <span className="rounded bg-terminal-warning/20 px-1 py-0 text-terminal-warning text-xs">
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
              loaded={activeFetchedOutput !== null}
              onError={(requestedFullOutputRef) => {
                if (!isCurrentFetch(requestedFullOutputRef)) return
                setFetchedOutput(null)
                setFetchFailed(true)
              }}
              onLoad={(response, requestedFullOutputRef) => {
                if (!isCurrentFetch(requestedFullOutputRef)) return
                setFetchFailed(false)
                setFetchedOutput(response)
              }}
              onStart={() => setFetchFailed(false)}
            />
          ) : null}
        </div>
      </div>
      {copyFailed ? (
        <div className="border-terminal-border border-b px-3 py-1 text-terminal-error text-xs">
          {t('timeline.commandEvidence.copyFailed', 'Copy failed')}
        </div>
      ) : null}
      {fetchFailed ? (
        <div className="border-terminal-border border-b px-3 py-1 text-terminal-error text-xs">
          {t('timeline.commandEvidence.fetchFailed', 'Failed to load output page')}
        </div>
      ) : null}

      {/* Command line */}
      <div className="border-terminal-border border-b px-3 py-2 font-mono text-terminal-foreground text-xs">
        $ {command.command}
      </div>

      {/* Metadata bar */}
      <div className="flex min-h-7 flex-wrap items-center gap-x-3 gap-y-1 border-terminal-border border-b px-3 py-1 text-xs">
        {command.cwd ? <span className="text-terminal-muted">{command.cwd}</span> : null}
        {command.shell ? <span className="text-terminal-muted">shell: {command.shell}</span> : null}
        {command.durationMs ? (
          <span className="text-terminal-muted">
            {t('timeline.commandEvidence.duration', { duration: command.durationMs })}
          </span>
        ) : null}
        {command.exitCode !== undefined ? (
          <span
            className={cn(
              command.exitCode === 0 ? 'text-terminal-foreground' : 'text-terminal-error',
            )}
          >
            {t('timeline.commandEvidence.exitCode', { code: command.exitCode })}
          </span>
        ) : null}
        {command.truncated ? (
          <span className="text-terminal-warning">{t('timeline.commandEvidence.truncated')}</span>
        ) : null}
        {activeFetchedOutput?.truncated ? (
          <span className="text-terminal-warning">
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
  onError: (requestedFullOutputRef: string) => void
  onLoad: (
    response: {
      hasMore: boolean
      nextCursor?: string
      output: string
      refId: string
      truncated: boolean
    },
    requestedFullOutputRef: string,
  ) => void
  onStart: () => void
}) {
  const { t } = useTranslation('conversation')
  const commandClient = useCommandClient()
  const [fetchingPage, setFetchingPage] = useState(false)

  const handleFetchPage = async () => {
    const requestedFullOutputRef = command.fullOutputRef
    if (!requestedFullOutputRef) return
    setFetchingPage(true)
    onStart()
    try {
      const response = await commandClient.getConversationCommandOutput({
        conversationId,
        fullOutputRef: requestedFullOutputRef,
      })
      if (response.refId !== requestedFullOutputRef || response.redactionState === 'withheld') {
        throw new Error('Command output withheld')
      }
      onLoad(
        {
          hasMore: response.hasMore,
          nextCursor: response.nextCursor,
          output: response.output,
          refId: response.refId,
          truncated: response.truncated,
        },
        requestedFullOutputRef,
      )
    } catch {
      onError(requestedFullOutputRef)
    } finally {
      setFetchingPage(false)
    }
  }

  return (
    <button
      className="rounded px-2 py-0.5 text-terminal-muted text-xs hover:bg-terminal-control hover:text-terminal-foreground focus-visible:ring-2 focus-visible:ring-ring"
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
      className="inline-flex size-6 items-center justify-center rounded text-terminal-muted hover:bg-terminal-control hover:text-terminal-foreground focus-visible:ring-2 focus-visible:ring-ring"
      onClick={onClick}
      type="button"
    >
      <Copy className="size-3" />
    </button>
  )
}
