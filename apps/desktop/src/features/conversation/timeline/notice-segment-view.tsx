import type { NoticeSegment } from '@/shared/tauri/commands'
import { ContextCompactionNotice } from './context-compaction-notice'

export function NoticeSegmentView({ segment }: { segment: NoticeSegment }) {
  if (segment.code === 'contextCompacted') {
    return <ContextCompactionNotice body={segment.body} />
  }

  return (
    <p className="rounded-md bg-muted px-3 py-2 text-muted-foreground text-sm">{segment.body}</p>
  )
}
