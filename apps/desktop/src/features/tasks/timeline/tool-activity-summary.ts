import type { TFunction } from 'i18next'

import type { TimelineItemProjection, TimelineToolOperation } from '@/generated/daemon-protocol'

export type ToolTimelineItem = TimelineItemProjection & {
  tool: NonNullable<TimelineItemProjection['tool']>
}

export function toolActivitySummary(item: TimelineItemProjection, t: TFunction<'tasks'>) {
  const tool = item.tool
  if (!tool) return item.summary
  const target = tool.subject ? ` ${tool.subject}` : ''
  const toolName = tool.operation === 'other' ? tool.toolName : ''
  if (tool.status === 'denied') return t('timeline.tool.denied', { tool: toolName })
  if (tool.status === 'failed') return t('timeline.tool.failed', { tool: toolName })
  const phase = tool.status === 'completed' ? 'completed' : 'active'
  return t(toolActionKey(phase, tool.operation), { target, tool: toolName })
}

export function toolActivityGroupSummary(items: ToolTimelineItem[], t: TFunction<'tasks'>) {
  if (items.length === 1) return toolActivitySummary(items[0], t)
  const active = items.filter((item) => ['requested', 'running'].includes(item.tool.status))
  if (active.length > 0) return t('timeline.tool.group.running', { count: items.length })
  const counts = new Map<TimelineToolOperation, number>()
  for (const item of items) {
    counts.set(item.tool.operation, (counts.get(item.tool.operation) ?? 0) + 1)
  }
  return [...counts.entries()]
    .map(([operation, count]) => t(toolGroupKey(operation), { count }))
    .join(t('timeline.tool.group.separator'))
}

function toolActionKey(phase: 'active' | 'completed', operation: TimelineToolOperation) {
  const keys = {
    active: {
      browse: 'timeline.tool.active.browse',
      command: 'timeline.tool.active.command',
      delegate: 'timeline.tool.active.delegate',
      edit: 'timeline.tool.active.edit',
      generate: 'timeline.tool.active.generate',
      other: 'timeline.tool.active.other',
      read: 'timeline.tool.active.read',
      search: 'timeline.tool.active.search',
    },
    completed: {
      browse: 'timeline.tool.completed.browse',
      command: 'timeline.tool.completed.command',
      delegate: 'timeline.tool.completed.delegate',
      edit: 'timeline.tool.completed.edit',
      generate: 'timeline.tool.completed.generate',
      other: 'timeline.tool.completed.other',
      read: 'timeline.tool.completed.read',
      search: 'timeline.tool.completed.search',
    },
  } as const
  return keys[phase][operation]
}

function toolGroupKey(operation: TimelineToolOperation) {
  const keys = {
    browse: 'timeline.tool.group.browse',
    command: 'timeline.tool.group.command',
    delegate: 'timeline.tool.group.delegate',
    edit: 'timeline.tool.group.edit',
    generate: 'timeline.tool.group.generate',
    other: 'timeline.tool.group.other',
    read: 'timeline.tool.group.read',
    search: 'timeline.tool.group.search',
  } as const
  return keys[operation]
}
