import { useTranslation } from 'react-i18next'
import { artifactDescriptorFromTimelineItem } from '@/features/artifacts/model'
import type { TimelineItemProjection } from '@/generated/daemon-protocol'

import { isLowValueLifecycleItem, TimelineEvent } from './TimelineEvent'
import { ToolActivityGroup } from './ToolActivityGroup'
import type { ToolTimelineItem } from './tool-activity-summary'

export function RunSegment({
  items,
  onSelectItem,
  segmentId,
}: {
  items: TimelineItemProjection[]
  onSelectItem?: (item: TimelineItemProjection, trigger?: HTMLElement) => void
  segmentId: string
}) {
  return (
    <div className="space-y-3" data-run-segment={segmentId}>
      {renderItems(items, onSelectItem)}
    </div>
  )
}

function renderItems(
  items: TimelineItemProjection[],
  onSelectItem?: (item: TimelineItemProjection, trigger?: HTMLElement) => void,
) {
  const rendered: React.ReactNode[] = []
  let index = 0

  while (index < items.length) {
    const item = items[index]
    if (!item) break
    if (isProjectedToolItem(item)) {
      const toolItems: ToolTimelineItem[] = []
      const linkedArtifacts: TimelineItemProjection[] = []
      while (items[index]) {
        const candidate = items[index] as TimelineItemProjection
        if (isProjectedToolItem(candidate)) {
          toolItems.push(candidate)
          index += 1
          continue
        }
        if (isLinkedFileArtifact(candidate, toolItems)) {
          linkedArtifacts.push(candidate)
          index += 1
          continue
        }
        break
      }
      rendered.push(
        <ToolActivityGroup
          artifacts={linkedArtifacts}
          items={toolItems}
          key={`tools:${toolItems[0]?.tool.toolUseId}`}
          onSelectItem={onSelectItem}
        />,
      )
      continue
    }
    if (isLowValueLifecycleItem(item)) {
      const lifecycleItems: TimelineItemProjection[] = []
      while (items[index] && isLowValueLifecycleItem(items[index] as TimelineItemProjection)) {
        lifecycleItems.push(items[index] as TimelineItemProjection)
        index += 1
      }
      rendered.push(
        <LifecycleSummary
          items={lifecycleItems}
          key={`lifecycle:${lifecycleItems[0]?.id}`}
          onSelectItem={onSelectItem}
        />,
      )
      continue
    }
    rendered.push(<TimelineEvent item={item} key={item.id} onSelect={onSelectItem} />)
    index += 1
  }
  return rendered
}

function LifecycleSummary({
  items,
  onSelectItem,
}: {
  items: TimelineItemProjection[]
  onSelectItem?: (item: TimelineItemProjection, trigger?: HTMLElement) => void
}) {
  const { t } = useTranslation('tasks')
  return (
    <details className="group rounded-md text-muted-foreground text-xs">
      <summary className="w-fit cursor-pointer select-none rounded-md px-2 py-1 hover:bg-muted/60 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring">
        {t('timeline.systemEvents', { count: items.length })}
      </summary>
      <div className="mt-1 space-y-1 border-border/70 border-l pl-2">
        {items.map((item) => (
          <TimelineEvent item={item} key={item.id} onSelect={onSelectItem} />
        ))}
      </div>
    </details>
  )
}

function isProjectedToolItem(item: TimelineItemProjection): item is ToolTimelineItem {
  return item.kind === 'tool_activity' && Boolean(item.tool)
}

function isLinkedFileArtifact(item: TimelineItemProjection, toolItems: ToolTimelineItem[]) {
  const artifact = artifactDescriptorFromTimelineItem(item)
  return Boolean(
    artifact?.sourceToolUseId &&
      ['diff', 'file', 'patch'].includes(artifact.artifactKind ?? '') &&
      toolItems.some((toolItem) => toolItem.tool.toolUseId === artifact.sourceToolUseId),
  )
}
