import { FileText, Folder } from 'lucide-react'
import { useTranslation } from 'react-i18next'

import type { SkillFile } from '@/shared/tauri/commands'

export type SkillFileSummary = {
  kind: SkillFile['kind']
  path: string
  sizeBytes?: number
}

export function buildSkillFileTree(files: SkillFileSummary[]): SkillFile[] {
  return files.map((file) => {
    const parts = file.path.split('/').filter(Boolean)
    return {
      depth: Math.max(0, parts.length - 1),
      kind: file.kind,
      name: parts.at(-1) ?? file.path,
      path: file.path,
      sizeBytes: file.sizeBytes,
    }
  })
}

export function SkillFileTree({
  files,
  onSelectFile,
  selectedFilePath,
}: {
  files: SkillFile[]
  onSelectFile: (path: string) => void
  selectedFilePath: string | null
}) {
  return files.map((file) => (
    <SkillFileRow
      file={file}
      key={file.path}
      onSelectFile={onSelectFile}
      selected={file.path === selectedFilePath}
    />
  ))
}

function SkillFileRow({
  file,
  onSelectFile,
  selected,
}: {
  file: SkillFile
  onSelectFile: (path: string) => void
  selected: boolean
}) {
  const { t } = useTranslation('skills')
  const indent = `${file.depth * 14}px`
  const icon =
    file.kind === 'directory' ? (
      <Folder data-icon className="size-4 text-muted-foreground" />
    ) : (
      <FileText data-icon className="size-4 text-muted-foreground" />
    )

  if (file.kind === 'directory') {
    return (
      <div
        className="flex h-8 items-center gap-2 rounded-sm px-2 text-muted-foreground text-sm"
        style={{ paddingLeft: `calc(0.5rem + ${indent})` }}
      >
        {icon}
        <span className="truncate">{file.name}</span>
      </div>
    )
  }

  return (
    <button
      aria-pressed={selected}
      className="flex h-8 w-full items-center gap-2 rounded-sm px-2 text-left text-sm outline-none hover:bg-muted focus-visible:ring-2 focus-visible:ring-ring aria-pressed:bg-muted"
      onClick={() => onSelectFile(file.path)}
      style={{ paddingLeft: `calc(0.5rem + ${indent})` }}
      type="button"
    >
      {icon}
      <span className="min-w-0 flex-1 truncate">{file.name}</span>
      {file.sizeBytes === undefined ? null : (
        <span className="text-muted-foreground text-xs">
          {t('files.size', { size: file.sizeBytes })}
        </span>
      )}
    </button>
  )
}
