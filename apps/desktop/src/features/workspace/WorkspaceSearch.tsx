import { Search } from 'lucide-react'
import type { ChangeEventHandler, RefObject } from 'react'
import { useTranslation } from 'react-i18next'

interface WorkspaceSearchProps {
  inputRef?: RefObject<HTMLInputElement | null>
  onChange: ChangeEventHandler<HTMLInputElement>
  value: string
}

export function WorkspaceSearch({ inputRef, onChange, value }: WorkspaceSearchProps) {
  const { t } = useTranslation('shell')

  return (
    <label className="relative block">
      <span className="sr-only">{t('search')}</span>
      <Search className="absolute top-1/2 left-3 size-4 -translate-y-1/2 text-muted-foreground" />
      <input
        className="h-9 w-full rounded-md border border-border bg-surface pr-12 pl-9 text-sm outline-none placeholder:text-muted-foreground focus-visible:ring-2 focus-visible:ring-ring"
        onChange={onChange}
        placeholder={t('search')}
        ref={inputRef}
        type="search"
        value={value}
      />
      <span
        aria-hidden="true"
        className="absolute top-1/2 right-3 -translate-y-1/2 rounded border border-border px-1.5 py-0.5 text-muted-foreground text-xs"
      >
        ⌘ K
      </span>
    </label>
  )
}
