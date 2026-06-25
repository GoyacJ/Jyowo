import type { ErrorSegment } from '@/shared/tauri/commands'

export function ErrorSegmentView({ segment }: { segment: ErrorSegment }) {
  return (
    <p className="rounded-md bg-destructive/10 px-3 py-2 text-destructive text-sm">
      {segment.body}
    </p>
  )
}
