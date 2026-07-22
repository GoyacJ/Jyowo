import { PanelLeftClose, PanelLeftOpen } from 'lucide-react'
import { type PointerEvent as ReactPointerEvent, useEffect, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'

import { cn } from '@/shared/lib/utils'
import {
  COLLAPSED_SIDEBAR_WIDTH,
  MAX_SIDEBAR_WIDTH,
  MIN_SIDEBAR_WIDTH,
} from '@/shared/state/sidebar-layout'
import { Button } from '@/shared/ui/button'

type SidebarDividerProps = {
  collapsed: boolean
  compact: boolean
  onCollapsedChange: (collapsed: boolean) => void
  onWidthChange: (width: number) => void
  width: number
}

type DragSession = {
  move: (event: PointerEvent) => void
  stop: (event: PointerEvent) => void
}

export function SidebarDivider({
  collapsed,
  compact,
  onCollapsedChange,
  onWidthChange,
  width,
}: SidebarDividerProps) {
  const { t } = useTranslation('shell')
  const dragSessionRef = useRef<DragSession | null>(null)
  const [dragging, setDragging] = useState(false)

  useEffect(
    () => () => {
      const session = dragSessionRef.current
      if (!session) return
      window.removeEventListener('pointermove', session.move)
      window.removeEventListener('pointerup', session.stop)
      window.removeEventListener('pointercancel', session.stop)
      dragSessionRef.current = null
    },
    [],
  )

  if (compact) return null

  function startResize(event: ReactPointerEvent<HTMLHRElement>) {
    if (collapsed || event.button !== 0) return

    event.preventDefault()
    const pointerId = event.pointerId
    const startWidth = width
    const startX = event.clientX
    const move = (moveEvent: PointerEvent) => {
      if (moveEvent.pointerId !== pointerId) return
      onWidthChange(startWidth + moveEvent.clientX - startX)
    }
    const stop = (stopEvent: PointerEvent) => {
      if (stopEvent.pointerId !== pointerId) return
      window.removeEventListener('pointermove', move)
      window.removeEventListener('pointerup', stop)
      window.removeEventListener('pointercancel', stop)
      dragSessionRef.current = null
      setDragging(false)
    }
    dragSessionRef.current = { move, stop }
    window.addEventListener('pointermove', move)
    window.addEventListener('pointerup', stop)
    window.addEventListener('pointercancel', stop)
    setDragging(true)
  }

  return (
    <div
      className="group pointer-events-auto absolute inset-y-0 z-20 w-2 -translate-x-1/2"
      data-dragging={dragging}
      style={{ left: collapsed ? COLLAPSED_SIDEBAR_WIDTH : width }}
    >
      <div
        aria-hidden="true"
        className={cn(
          'absolute inset-y-0 left-1/2 w-px -translate-x-1/2 bg-border transition-colors',
          'group-focus-within:bg-ring/70 group-hover:bg-ring/70',
          dragging && 'bg-ring',
        )}
      />
      <hr
        aria-disabled={collapsed}
        aria-label={t('actions.resizeSidebar')}
        aria-orientation="vertical"
        aria-valuemax={MAX_SIDEBAR_WIDTH}
        aria-valuemin={MIN_SIDEBAR_WIDTH}
        aria-valuenow={width}
        className={cn(
          'pointer-events-auto absolute inset-0 m-0 h-full border-0 focus-visible:outline-none',
          collapsed ? 'cursor-default' : 'cursor-col-resize',
        )}
        onKeyDown={(event) => {
          if (collapsed) return
          if (event.key === 'ArrowLeft') onWidthChange(width - 10)
          else if (event.key === 'ArrowRight') onWidthChange(width + 10)
          else if (event.key === 'Home') onWidthChange(MIN_SIDEBAR_WIDTH)
          else if (event.key === 'End') onWidthChange(MAX_SIDEBAR_WIDTH)
          else return
          event.preventDefault()
        }}
        onPointerDown={startResize}
        tabIndex={collapsed ? -1 : 0}
      />
      {!dragging ? (
        <Button
          aria-label={collapsed ? t('actions.expandSidebar') : t('actions.collapseSidebar')}
          className={cn(
            'pointer-events-none absolute top-1/2 left-1/2 size-7 -translate-x-1/2 -translate-y-1/2 bg-background opacity-0 transition-opacity',
            'group-focus-within:pointer-events-auto group-focus-within:opacity-100 group-hover:pointer-events-auto group-hover:opacity-100',
          )}
          onClick={() => onCollapsedChange(!collapsed)}
          onPointerDown={(event) => event.stopPropagation()}
          size="icon"
          title={collapsed ? t('actions.expandSidebar') : t('actions.collapseSidebar')}
          type="button"
          variant="outline"
        >
          {collapsed ? (
            <PanelLeftOpen aria-hidden="true" className="size-4" />
          ) : (
            <PanelLeftClose aria-hidden="true" className="size-4" />
          )}
        </Button>
      ) : null}
    </div>
  )
}
