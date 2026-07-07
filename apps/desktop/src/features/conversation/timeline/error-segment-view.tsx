import type { ErrorSegment } from '@/shared/tauri/commands'

import { RedactedBody } from './redacted-body'

export function ErrorSegmentView({ segment }: { segment: ErrorSegment }) {
  if (segment.redactedOriginalBody) {
    return (
      <div className="rounded-md bg-destructive/10 px-3 py-2">
        <RedactedBody
          className="text-destructive text-sm"
          originalBody={segment.redactedOriginalBody}
        />
      </div>
    )
  }

  return (
    <p className="rounded-md bg-destructive/10 px-3 py-2 text-destructive text-sm">
      {segment.body}
    </p>
  )
}
