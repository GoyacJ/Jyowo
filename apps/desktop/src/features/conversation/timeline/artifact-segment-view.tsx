import type { ArtifactSegment } from '@/shared/tauri/commands'

export function ArtifactSegmentView({ segment }: { segment: ArtifactSegment }) {
  return (
    <section className="rounded-md border border-border px-3 py-2">
      <div className="font-medium text-sm">{segment.title}</div>
      {segment.summary ? (
        <p className="mt-1 text-muted-foreground text-sm">{segment.summary}</p>
      ) : null}
    </section>
  )
}
