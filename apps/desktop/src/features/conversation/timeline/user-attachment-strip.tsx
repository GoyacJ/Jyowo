import { FileText, Image as ImageIcon } from 'lucide-react'

import type { AttachmentReference } from '@/shared/tauri/commands'

export function UserAttachmentStrip({ attachments }: { attachments: AttachmentReference[] }) {
  if (attachments.length === 0) {
    return null
  }

  return (
    <div className="w-full overflow-x-auto pb-1 [scrollbar-width:thin]">
      <ul className="ml-auto flex w-max max-w-none gap-2" aria-label="User attachments">
        {attachments.map((attachment) => (
          <li key={attachment.id}>
            <AttachmentChip attachment={attachment} />
          </li>
        ))}
      </ul>
    </div>
  )
}

function AttachmentChip({ attachment }: { attachment: AttachmentReference }) {
  const isImage = attachment.mimeType.startsWith('image/')
  const Icon = isImage ? ImageIcon : FileText

  return (
    <div
      className="grid h-14 w-40 grid-cols-[32px_minmax(0,1fr)] items-center gap-2 rounded-md border border-border bg-surface px-2 shadow-sm sm:h-[72px] sm:w-48"
      title={`${attachment.name} · ${attachment.mimeType} · ${formatBytes(attachment.sizeBytes)}`}
    >
      <div className="flex size-8 items-center justify-center rounded border border-border bg-muted text-muted-foreground">
        <Icon aria-hidden="true" className="size-4" />
      </div>
      <div className="min-w-0">
        <div className="truncate font-medium text-foreground text-xs">{attachment.name}</div>
        <div className="mt-0.5 truncate text-muted-foreground text-[11px] leading-4">
          {attachment.mimeType}
        </div>
        <div className="text-muted-foreground text-[11px] leading-4">
          {formatBytes(attachment.sizeBytes)}
        </div>
      </div>
    </div>
  )
}

function formatBytes(sizeBytes: number) {
  if (sizeBytes < 1024) {
    return `${sizeBytes} B`
  }
  if (sizeBytes < 1024 * 1024) {
    return `${formatUnit(sizeBytes / 1024)} KB`
  }
  return `${formatUnit(sizeBytes / (1024 * 1024))} MB`
}

function formatUnit(value: number) {
  return Number.isInteger(value) ? String(value) : value.toFixed(1)
}
