import { CircleAlert, Code2, LoaderCircle, Play, RefreshCw, Square } from 'lucide-react'
import { useCallback, useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'

import type { RuntimeCommand, ServerMessage, TypedUlid } from '@/generated/daemon-protocol'
import type { DaemonClient } from '@/shared/daemon/client'
import { Button } from '@/shared/ui/button'

type RuntimeSession = Extract<ServerMessage, { type: 'runtime_session' }>

export function HtmlRuntimePanel({
  blobId,
  client,
  source,
  taskId,
  title,
}: {
  blobId: TypedUlid
  client: Pick<DaemonClient, 'request'>
  source: React.ReactNode
  taskId: TypedUlid
  title: string
}) {
  const { t } = useTranslation('tasks')
  const [failed, setFailed] = useState(false)
  const [frameKey, setFrameKey] = useState(0)
  const [frameLoaded, setFrameLoaded] = useState(false)
  const [pending, setPending] = useState(true)
  const [session, setSession] = useState<RuntimeSession | null>(null)
  const [showSource, setShowSource] = useState(false)
  const sessionId = `html-${blobId}`
  const runtimeTitle = title.trim() || t('workbench.runtime.untitled')

  const send = useCallback(
    async (command: RuntimeCommand) => {
      const frame = await client.request({ command, taskId, type: 'runtime' })
      if (frame.message.type === 'error') throw new Error(frame.message.message)
      if (frame.message.type !== 'runtime_session') {
        throw new Error(`Expected runtime_session, received ${frame.message.type}`)
      }
      if (frame.message.taskId !== taskId || frame.message.kind !== 'html') {
        throw new Error('HTML runtime session identity does not match the request')
      }
      return frame.message
    },
    [client, taskId],
  )

  useEffect(() => {
    let cancelled = false
    setPending(true)
    setFailed(false)
    void send({ kind: 'html', sessionId, type: 'status' })
      .then((current) => {
        if (!cancelled) setSession(current)
      })
      .catch(() => {
        if (!cancelled) setFailed(true)
      })
      .finally(() => {
        if (!cancelled) setPending(false)
      })
    return () => {
      cancelled = true
    }
  }, [send, sessionId])

  const viewUrl = safeHtmlRuntimeUrl(session?.view)
  const ready = session?.status === 'ready' && viewUrl !== null

  async function start() {
    setPending(true)
    setFailed(false)
    try {
      const next = await send({
        spec: { blobId, kind: 'html', title: runtimeTitle },
        type: 'open',
      })
      setSession(next)
      setShowSource(false)
      setFrameLoaded(false)
      setFrameKey((current) => current + 1)
    } catch {
      setFailed(true)
    } finally {
      setPending(false)
    }
  }

  async function stop() {
    setPending(true)
    setFailed(false)
    try {
      setSession(await send({ kind: 'html', sessionId, type: 'close' }))
      setShowSource(false)
      setFrameLoaded(false)
    } catch {
      setFailed(true)
    } finally {
      setPending(false)
    }
  }

  if (!ready || showSource) {
    return (
      <div className="flex h-full min-h-0 flex-col">
        <RuntimeToolbar
          failed={failed}
          onRefresh={() => void start()}
          onShowSource={ready ? () => setShowSource(false) : undefined}
          onStart={() => void start()}
          onStop={ready ? () => void stop() : undefined}
          pending={pending}
          ready={ready}
          showingSource={showSource}
          title={runtimeTitle}
        />
        {failed ? (
          <div
            className="flex items-center gap-2 border-border border-b bg-destructive/5 px-3 py-2 text-destructive text-xs"
            role="alert"
          >
            <CircleAlert aria-hidden="true" className="size-4 shrink-0" />
            {t('workbench.runtime.failedDescription')}
          </div>
        ) : null}
        <div className="min-h-0 flex-1 overflow-auto">{source}</div>
      </div>
    )
  }

  return (
    <div className="flex h-full min-h-0 flex-col">
      <RuntimeToolbar
        failed={failed}
        onRefresh={() => {
          setFrameLoaded(false)
          setFrameKey((current) => current + 1)
        }}
        onShowSource={() => setShowSource(true)}
        onStart={() => void start()}
        onStop={() => void stop()}
        pending={pending}
        ready
        showingSource={false}
        title={session.title}
      />
      <div className="relative min-h-0 flex-1 bg-background">
        {!frameLoaded ? (
          <div
            className="absolute inset-0 z-10 grid place-items-center bg-background text-muted-foreground"
            role="status"
          >
            <div className="flex items-center gap-2 text-xs">
              <LoaderCircle aria-hidden="true" className="size-4 animate-spin" />
              {t('workbench.runtime.loading')}
            </div>
          </div>
        ) : null}
        <iframe
          className="h-full w-full border-0 bg-background"
          key={frameKey}
          onLoad={() => setFrameLoaded(true)}
          referrerPolicy="no-referrer"
          sandbox="allow-forms allow-scripts"
          src={viewUrl}
          title={t('workbench.runtime.frameTitle', { title: session.title })}
        />
      </div>
    </div>
  )
}

function RuntimeToolbar({
  failed,
  onRefresh,
  onShowSource,
  onStart,
  onStop,
  pending,
  ready,
  showingSource,
  title,
}: {
  failed: boolean
  onRefresh: () => void
  onShowSource?: () => void
  onStart: () => void
  onStop?: () => void
  pending: boolean
  ready: boolean
  showingSource: boolean
  title: string
}) {
  const { t } = useTranslation('tasks')
  return (
    <div className="flex h-10 shrink-0 items-center gap-2 border-border border-b bg-background px-3">
      <span
        aria-hidden="true"
        className={`size-2 shrink-0 rounded-full ${ready ? 'bg-success' : 'bg-muted-foreground/40'}`}
      />
      <p className="min-w-0 flex-1 truncate font-medium text-[11px]">{title}</p>
      {ready ? (
        <Button
          aria-label={
            showingSource ? t('workbench.runtime.showRuntime') : t('workbench.runtime.showSource')
          }
          className="size-7"
          onClick={onShowSource}
          size="icon"
          type="button"
          variant="ghost"
        >
          {showingSource ? (
            <Play aria-hidden="true" className="size-3.5" />
          ) : (
            <Code2 aria-hidden="true" className="size-3.5" />
          )}
        </Button>
      ) : null}
      {ready && !showingSource ? (
        <Button
          aria-label={t('workbench.runtime.reload')}
          className="size-7"
          disabled={pending}
          onClick={onRefresh}
          size="icon"
          type="button"
          variant="ghost"
        >
          <RefreshCw aria-hidden="true" className="size-3.5" />
        </Button>
      ) : null}
      {ready ? (
        <Button disabled={pending} onClick={onStop} size="sm" type="button" variant="outline">
          {pending ? (
            <LoaderCircle aria-hidden="true" className="size-3.5 animate-spin" />
          ) : (
            <Square aria-hidden="true" className="size-3.5" />
          )}
          {t('workbench.runtime.stop')}
        </Button>
      ) : (
        <Button disabled={pending} onClick={onStart} size="sm" type="button">
          {pending ? (
            <LoaderCircle aria-hidden="true" className="size-3.5 animate-spin" />
          ) : failed ? (
            <RefreshCw aria-hidden="true" className="size-3.5" />
          ) : (
            <Play aria-hidden="true" className="size-3.5" />
          )}
          {failed ? t('workbench.runtime.retry') : t('workbench.runtime.run')}
        </Button>
      )}
    </div>
  )
}

function safeHtmlRuntimeUrl(view: RuntimeSession['view']) {
  if (view?.type !== 'url') return null
  try {
    const url = new URL(view.url)
    if (
      url.protocol !== 'http:' ||
      url.hostname !== '127.0.0.1' ||
      !url.port ||
      !url.pathname.startsWith('/preview/html-')
    ) {
      return null
    }
    return url.toString()
  } catch {
    return null
  }
}
