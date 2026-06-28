import { MarkdownMessage } from '@/shared/markdown/MarkdownMessage'
import type { TextSegment } from '@/shared/tauri/commands'

export function AssistantTextSegmentView({ segment }: { segment: TextSegment }) {
  if (!segment.body.trim()) {
    return null
  }

  return <MarkdownMessage className="text-foreground">{segment.body}</MarkdownMessage>
}
