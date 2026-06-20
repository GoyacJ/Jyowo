import { FileText } from 'lucide-react'
import { useTranslation } from 'react-i18next'

export type ContextFileReference = {
  label: string
  state?: 'ready' | 'missing' | 'stale'
}

type FileReferenceListProps = {
  files: ContextFileReference[]
}

export function FileReferenceList({ files }: FileReferenceListProps) {
  const { t } = useTranslation('context')

  if (files.length === 0) {
    return <p className="text-muted-foreground text-sm">{t('noFiles')}</p>
  }

  return (
    <ul aria-label={t('files')} className="space-y-2">
      {files.map((file) => {
        const stateLabel = getStateLabel(file.state ?? 'ready', t)
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

function getStateLabel(
  state: NonNullable<ContextFileReference['state']>,
  t: (key: string) => string,
) {
  if (state === 'missing') {
    return t('missing')
  }

  if (state === 'stale') {
    return t('stale')
  }

  return ''
}
