import { Eye, Trash2 } from 'lucide-react'
import { useTranslation } from 'react-i18next'
import { Button } from '@/shared/ui/button'
import type { MemoryItemSummary } from './memory-types'

interface MemoryItemCardProps {
  item: MemoryItemSummary
  onDelete: (id: string) => void
  onInspect: (id: string) => void
}

export function MemoryItemCard({ item, onDelete, onInspect }: MemoryItemCardProps) {
  const { t } = useTranslation('memory')

  return (
    <article
      aria-label={`Memory ${item.id}`}
      className="rounded-md border border-border bg-surface p-3"
    >
      <div className="flex items-start justify-between gap-3">
        <div className="min-w-0 space-y-2">
          <p className="line-clamp-3 text-sm">{item.contentPreview}</p>
          <div className="flex flex-wrap gap-2 text-muted-foreground text-xs">
            <span>{item.visibility}</span>
            <span>{item.kind}</span>
            {item.providerId ? <span>{item.providerId}</span> : null}
            {item.expiresAt ? <span>{t('expiresAt', { value: item.expiresAt })}</span> : null}
            {item.lastAccessedAt ? (
              <span>{t('lastAccessedAt', { value: item.lastAccessedAt })}</span>
            ) : null}
            {item.deleted ? <span>{t('deleted')}</span> : null}
            <span>{t('contentHash', { value: item.contentHash.slice(0, 12) })}</span>
            {item.tags.map((tag) => (
              <span key={tag}>{tag}</span>
            ))}
          </div>
        </div>
        <div className="flex shrink-0 gap-1">
          <Button
            aria-label={t('inspect')}
            onClick={() => onInspect(item.id)}
            size="icon"
            type="button"
            variant="ghost"
          >
            <Eye data-icon className="size-4" />
          </Button>
          <Button
            aria-label={t('delete')}
            onClick={() => onDelete(item.id)}
            size="icon"
            type="button"
            variant="ghost"
          >
            <Trash2 data-icon className="size-4" />
          </Button>
        </div>
      </div>
    </article>
  )
}
