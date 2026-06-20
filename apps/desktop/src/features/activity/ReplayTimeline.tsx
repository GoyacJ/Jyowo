import { useVirtualizer } from '@tanstack/react-virtual'
import type { CSSProperties } from 'react'
import { useRef } from 'react'
import { useTranslation } from 'react-i18next'

import type { RunEvent } from '@/shared/events/run-event-schema'

import { getActivityStatusClass } from './ActivityItem'
import { type RunEventViewModel, toRunEventViewModels } from './run-event-view-model'

type ReplayTimelineProps = {
  errorMessage?: string
  events: RunEvent[]
  loading?: boolean
  replayed: boolean
}

const virtualTimelineThreshold = 100
const virtualRowHeight = 48

type VirtualReplayItem = {
  index: number
  key: number | string
  start: number
}

export function ReplayTimeline({
  errorMessage,
  events,
  loading = false,
  replayed,
}: ReplayTimelineProps) {
  const { t } = useTranslation('activity')
  const viewModels = toRunEventViewModels(events)

  return (
    <section aria-label={t('replay.title')} className="space-y-4">
      <header className="flex items-center gap-3">
        <h2 className="font-medium text-sm">{t('replay.title')}</h2>
        {replayed ? (
          <span className="rounded-md border border-border bg-surface px-2 py-1 text-muted-foreground text-xs">
            {t('replay.readOnly')}
          </span>
        ) : null}
      </header>

      {loading ? (
        <p className="text-muted-foreground text-sm">{t('replay.loading')}</p>
      ) : errorMessage ? (
        <p className="text-destructive text-sm">{errorMessage}</p>
      ) : viewModels.length === 0 ? (
        <p className="text-muted-foreground text-sm">{t('replay.empty')}</p>
      ) : viewModels.length > virtualTimelineThreshold ? (
        <VirtualReplayTimeline viewModels={viewModels} />
      ) : (
        <ol className="space-y-2">
          {viewModels.map((viewModel) => (
            <ReplayTimelineRow key={viewModel.activityItem.id} viewModel={viewModel} />
          ))}
        </ol>
      )}
    </section>
  )
}

function VirtualReplayTimeline({ viewModels }: { viewModels: RunEventViewModel[] }) {
  const { t } = useTranslation('activity')
  const parentRef = useRef<HTMLDivElement | null>(null)
  const virtualizer = useVirtualizer({
    count: viewModels.length,
    estimateSize: () => virtualRowHeight,
    getScrollElement: () => parentRef.current,
    initialRect: {
      height: 360,
      width: 720,
    },
    overscan: 4,
  })
  const virtualItems = virtualizer.getVirtualItems()
  const visibleItems: VirtualReplayItem[] =
    virtualItems.length > 0
      ? virtualItems.map((virtualItem) => ({
          index: virtualItem.index,
          key: String(virtualItem.key),
          start: virtualItem.start,
        }))
      : Array.from({ length: Math.min(12, viewModels.length) }, (_, index) => ({
          index,
          key: viewModels[index].activityItem.id,
          start: index * virtualRowHeight,
        }))

  return (
    <div className="space-y-2">
      <p className="text-muted-foreground text-xs">
        {t('replay.events', { count: viewModels.length })}
      </p>
      <div
        className="relative h-[360px] overflow-y-auto"
        ref={parentRef}
        style={{ contain: 'strict' }}
      >
        <ol
          aria-label={t('replay.virtualizedEvents')}
          className="relative w-full"
          style={{
            height: `${virtualizer.getTotalSize()}px`,
          }}
        >
          {visibleItems.map((virtualItem) => (
            <ReplayTimelineRow
              ariaPosInSet={virtualItem.index + 1}
              ariaSetSize={viewModels.length}
              key={virtualItem.key}
              measureElement={virtualizer.measureElement}
              style={{
                left: 0,
                paddingBottom: '0.5rem',
                position: 'absolute',
                top: 0,
                transform: `translateY(${virtualItem.start}px)`,
                width: '100%',
              }}
              viewModel={viewModels[virtualItem.index]}
            />
          ))}
        </ol>
      </div>
    </div>
  )
}

function ReplayTimelineRow({
  ariaPosInSet,
  ariaSetSize,
  measureElement,
  style,
  viewModel,
}: {
  ariaPosInSet?: number
  ariaSetSize?: number
  measureElement?: (node: Element | null) => void
  style?: CSSProperties
  viewModel: RunEventViewModel
}) {
  const { t } = useTranslation(['activity', 'common'])
  const statusClass = getActivityStatusClass(viewModel.activityItem.status)
  const labelKey = getActivityLabelKey(viewModel.activityItem.label)

  return (
    <li
      aria-posinset={ariaPosInSet}
      aria-setsize={ariaSetSize}
      className="rounded-md border border-border bg-surface px-3 py-2 text-sm"
      data-index={ariaPosInSet === undefined ? undefined : ariaPosInSet - 1}
      ref={measureElement}
      style={style}
    >
      <div className="flex flex-wrap items-center gap-x-4 gap-y-1">
        <span className="font-mono text-muted-foreground text-xs">{viewModel.order.sequence}</span>
        <span>
          {viewModel.rawJson?.withheld
            ? t('activity:replay.withheldEvent')
            : labelKey
              ? t(`activity:${labelKey}`)
              : viewModel.activityItem.label}
        </span>
        <span className={statusClass}>{t(`common:status.${viewModel.activityItem.status}`)}</span>
        <time className="text-muted-foreground text-xs">{viewModel.order.timestamp}</time>
      </div>
    </li>
  )
}

function getActivityLabelKey(label: string): string | undefined {
  switch (label) {
    case 'assistant':
    case 'engine':
    case 'permission':
    case 'run':
    case 'tool':
      return `eventLabels.${label}`
    default:
      return undefined
  }
}
