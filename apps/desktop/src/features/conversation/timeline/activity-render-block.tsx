import { FileText, type LucideIcon, Search, Terminal, Wrench } from 'lucide-react'
import { useTranslation } from 'react-i18next'
import { EvidenceDisclosure } from './evidence-disclosure'
import { useTimelineBlockDisclosure } from './timeline-disclosure-state'
import type { TimelineRenderBlock } from './timeline-render-blocks'

type ActivityBlock = Extract<TimelineRenderBlock, { kind: 'activity' }>

export function ActivityRenderBlock({
  block,
  conversationId,
  runId,
}: {
  block: ActivityBlock
  conversationId: string
  runId: string
}) {
  const { t } = useTranslation('conversation')
  const { open, setOpen } = useTimelineBlockDisclosure({ block, conversationId, runId })
  const count = block.itemCount ?? block.items.length

  return (
    <EvidenceDisclosure
      forcedOpen={block.forcedOpen}
      icon={Search}
      id={block.id}
      meta={block.title}
      onOpenChange={setOpen}
      open={open}
      title={t('timeline.renderBlocks.activitySummary', { count })}
    >
      {block.items.length > 0 ? (
        <ul className="grid gap-1.5">
          {block.items.map((item) => {
            const Icon = activityItemIcon(item.kind)
            return (
              <li className="flex min-w-0 items-start gap-2 text-sm" key={item.id}>
                <Icon className="mt-0.5 size-3.5 shrink-0 text-muted-foreground" />
                <span className="min-w-0 flex-1 truncate">{item.label}</span>
                {item.detail ? (
                  <span className="max-w-[40%] shrink truncate text-muted-foreground text-xs">
                    {item.detail}
                  </span>
                ) : null}
              </li>
            )
          })}
        </ul>
      ) : (
        <p className="text-muted-foreground text-sm">
          {block.title || t('timeline.renderBlocks.activityFallback')}
        </p>
      )}
    </EvidenceDisclosure>
  )
}

function activityItemIcon(kind: ActivityBlock['items'][number]['kind']): LucideIcon {
  switch (kind) {
    case 'file':
      return FileText
    case 'search':
      return Search
    case 'command':
      return Terminal
    case 'tool':
      return Wrench
  }
}
