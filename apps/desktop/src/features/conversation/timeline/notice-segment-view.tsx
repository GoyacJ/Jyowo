import type { NoticeSegment } from '@/shared/tauri/commands'

export function NoticeSegmentView({ segment }: { segment: NoticeSegment }) {
  return (
    <p className="rounded-md bg-muted px-3 py-2 text-muted-foreground text-sm">{segment.body}</p>
  )
}
