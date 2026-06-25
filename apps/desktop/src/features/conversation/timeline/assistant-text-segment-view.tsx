import type { TextSegment } from '@/shared/tauri/commands'

export function AssistantTextSegmentView({ segment }: { segment: TextSegment }) {
  if (!segment.body.trim()) {
    return null
  }

  return <p className="whitespace-pre-wrap text-sm leading-6">{segment.body}</p>
}
