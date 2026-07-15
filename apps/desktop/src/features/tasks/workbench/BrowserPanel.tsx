import { CircleAlert, Globe2, LoaderCircle, Power, RefreshCw } from 'lucide-react'
import { useCallback, useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'

import type { BrowserCommand, ServerMessage, TypedUlid } from '@/generated/daemon-protocol'
import type { DaemonClient } from '@/shared/daemon/client'
import { Button } from '@/shared/ui/button'

type BrowserSession = Extract<ServerMessage, { type: 'browser_session' }>

export function BrowserPanel({
  client,
  taskId,
}: {
  client: Pick<DaemonClient, 'request'>
  taskId: TypedUlid
}) {
  const { t } = useTranslation('tasks')
  const [session, setSession] = useState<BrowserSession | null>(null)
  const [failed, setFailed] = useState(false)
  const [pending, setPending] = useState(true)
  const [frameLoaded, setFrameLoaded] = useState(false)
  const pageVisible = usePageVisible()

  const send = useCallback(
    async (command: BrowserCommand) => {
      const frame = await client.request({ command, taskId, type: 'browser' })
      if (frame.message.type === 'error') throw new Error(frame.message.message)
      if (frame.message.type !== 'browser_session') {
        throw new Error(`Expected browser_session, received ${frame.message.type}`)
      }
      if (frame.message.taskId !== taskId)
        throw new Error('Browser session belongs to another task')
      return frame.message
    },
    [client, taskId],
  )

  const start = useCallback(async () => {
    setPending(true)
    setFailed(false)
    try {
      setSession(await send({ type: 'open' }))
    } catch {
      setFailed(true)
    } finally {
      setPending(false)
    }
  }, [send])

  useEffect(() => {
    let cancelled = false
    setPending(true)
    setFailed(false)
    void send({ type: 'status' })
      .then(async (current) => {
        if (cancelled) return
        const next = current.status === 'stopped' ? await send({ type: 'open' }) : current
        if (!cancelled) setSession(next)
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
  }, [send])

  useEffect(() => {
    if (!pageVisible || session?.status !== 'ready') return
    const interval = window.setInterval(() => {
      void send({ type: 'status' })
        .then(setSession)
        .catch(() => setFailed(true))
    }, 4_000)
    return () => window.clearInterval(interval)
  }, [pageVisible, send, session?.status])

  const dashboardUrl = safeDashboardUrl(session?.dashboardUrl)
  const ready = session?.status === 'ready' && dashboardUrl !== null
  const unavailable = session?.status === 'unavailable'
  const stopped = session?.status === 'stopped'
  const sessionFailed =
    session?.status === 'failed' || (session?.status === 'ready' && !dashboardUrl)

  useEffect(() => {
    if (pageVisible) setFrameLoaded(false)
  }, [dashboardUrl, pageVisible])

  async function stop() {
    setPending(true)
    setFailed(false)
    try {
      setSession(await send({ type: 'close' }))
      setFrameLoaded(false)
    } catch {
      setFailed(true)
    } finally {
      setPending(false)
    }
  }

  async function refresh() {
    setPending(true)
    setFailed(false)
    try {
      setSession(await send({ type: 'status' }))
    } catch {
      setFailed(true)
    } finally {
      setPending(false)
    }
  }

  if (pending && !ready) {
    return (
      <BrowserState
        description={t('workbench.browser.startingDescription')}
        icon={<LoaderCircle aria-hidden="true" className="size-5 animate-spin" />}
        title={t('workbench.browser.starting')}
      />
    )
  }

  if (unavailable) {
    return (
      <BrowserState
        action={
          <Button onClick={() => void refresh()} size="sm" type="button" variant="outline">
            <RefreshCw aria-hidden="true" className="size-3.5" />
            {t('workbench.browser.retry')}
          </Button>
        }
        description={session.unavailableReason ?? t('workbench.browser.unavailableDescription')}
        icon={<CircleAlert aria-hidden="true" className="size-5" />}
        title={t('workbench.browser.unavailable')}
      />
    )
  }

  if (failed || sessionFailed) {
    return (
      <BrowserState
        action={
          <Button onClick={() => void start()} size="sm" type="button" variant="outline">
            <RefreshCw aria-hidden="true" className="size-3.5" />
            {t('workbench.browser.retry')}
          </Button>
        }
        description={session?.unavailableReason ?? t('workbench.browser.failedDescription')}
        icon={<CircleAlert aria-hidden="true" className="size-5" />}
        title={t('workbench.browser.failed')}
      />
    )
  }

  if (stopped || !session) {
    return (
      <BrowserState
        action={
          <Button onClick={() => void start()} size="sm" type="button">
            <Globe2 aria-hidden="true" className="size-3.5" />
            {t('workbench.browser.start')}
          </Button>
        }
        description={t('workbench.browser.stoppedDescription')}
        icon={<Power aria-hidden="true" className="size-5" />}
        title={t('workbench.browser.stopped')}
      />
    )
  }

  if (!ready) {
    return (
      <BrowserState
        description={t('workbench.browser.startingDescription')}
        icon={<LoaderCircle aria-hidden="true" className="size-5 animate-spin" />}
        title={t('workbench.browser.starting')}
      />
    )
  }

  return (
    <div className="flex h-full min-h-[360px] flex-col bg-muted/10">
      <div className="flex h-10 shrink-0 items-center gap-2 border-border border-b bg-background px-3">
        <span aria-hidden="true" className="size-2 shrink-0 rounded-full bg-emerald-500" />
        <div className="min-w-0 flex-1">
          <p className="truncate font-medium text-[11px]">
            {session.title || t('workbench.browser.title')}
          </p>
          <p className="truncate font-mono text-[9px] text-muted-foreground">
            {session.currentUrl || 'about:blank'}
          </p>
        </div>
        <Button
          aria-label={t('workbench.browser.refresh')}
          className="size-7"
          disabled={pending}
          onClick={() => void refresh()}
          size="icon"
          type="button"
          variant="ghost"
        >
          <RefreshCw aria-hidden="true" className={`size-3.5 ${pending ? 'animate-spin' : ''}`} />
        </Button>
        <Button
          aria-label={t('workbench.browser.close')}
          className="size-7"
          disabled={pending}
          onClick={() => void stop()}
          size="icon"
          type="button"
          variant="ghost"
        >
          <Power aria-hidden="true" className="size-3.5" />
        </Button>
      </div>

      <div className="relative min-h-0 flex-1 bg-background">
        {pageVisible ? (
          <>
            {!frameLoaded ? (
              <div
                className="absolute inset-0 z-10 grid place-items-center bg-background text-muted-foreground"
                role="status"
              >
                <div className="flex items-center gap-2 text-xs">
                  <LoaderCircle aria-hidden="true" className="size-4 animate-spin" />
                  {t('workbench.browser.loadingPage')}
                </div>
              </div>
            ) : null}
            <iframe
              allow="clipboard-read; clipboard-write"
              className="h-full w-full border-0 bg-background"
              onLoad={() => setFrameLoaded(true)}
              referrerPolicy="no-referrer"
              sandbox="allow-downloads allow-forms allow-pointer-lock allow-same-origin allow-scripts"
              src={dashboardUrl}
              title={t('workbench.browser.frameTitle')}
            />
          </>
        ) : (
          <BrowserState
            description={t('workbench.browser.pausedDescription')}
            icon={<Globe2 aria-hidden="true" className="size-5" />}
            title={t('workbench.browser.paused')}
          />
        )}
      </div>
    </div>
  )
}

function BrowserState({
  action,
  description,
  icon,
  title,
}: {
  action?: React.ReactNode
  description: string
  icon: React.ReactNode
  title: string
}) {
  return (
    <div className="grid h-full min-h-64 place-items-center bg-muted/10 p-6">
      <div className="flex max-w-sm flex-col items-center gap-3 text-center">
        <div className="rounded-full border border-border bg-background p-3 text-muted-foreground">
          {icon}
        </div>
        <div>
          <p className="font-medium text-sm">{title}</p>
          <p className="mt-1 text-muted-foreground text-xs leading-5">{description}</p>
        </div>
        {action}
      </div>
    </div>
  )
}

function safeDashboardUrl(value: string | null | undefined) {
  if (!value) return null
  try {
    const url = new URL(value)
    if (url.protocol !== 'http:' || url.hostname !== '127.0.0.1' || !url.port) return null
    return url.toString()
  } catch {
    return null
  }
}

function usePageVisible() {
  const [visible, setVisible] = useState(() => document.visibilityState !== 'hidden')
  useEffect(() => {
    const update = () => setVisible(document.visibilityState !== 'hidden')
    document.addEventListener('visibilitychange', update)
    return () => document.removeEventListener('visibilitychange', update)
  }, [])
  return visible
}
