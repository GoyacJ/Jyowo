import { FileText } from 'lucide-react'

export type ContextFileReference = {
  label: string
  state?: 'ready' | 'missing' | 'stale'
}

type FileReferenceListProps = {
  files: ContextFileReference[]
}

const stateLabels = {
  missing: 'Missing',
  ready: '',
  stale: 'Stale',
} satisfies Record<NonNullable<ContextFileReference['state']>, string>

export function FileReferenceList({ files }: FileReferenceListProps) {
  if (files.length === 0) {
    return <p className="text-muted-foreground text-sm">No files attached.</p>
  }

  return (
    <ul aria-label="Files" className="space-y-2">
      {files.map((file) => {
        const stateLabel = stateLabels[file.state ?? 'ready']
        const accessibleName = stateLabel ? `${file.label} ${stateLabel}` : file.label

        return (
          <li
            aria-label={accessibleName}
            className="flex min-w-0 items-center gap-2"
            key={file.label}
          >
            <FileText className="size-4 shrink-0 text-muted-foreground" />
            <span className="min-w-0 truncate">{file.label}</span>
            {stateLabel ? (
              <span className="ml-auto rounded border border-border px-1.5 py-0.5 text-muted-foreground text-xs">
                {stateLabel}
              </span>
            ) : null}
          </li>
        )
      })}
    </ul>
  )
}
