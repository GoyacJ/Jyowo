import { createLazyFileRoute } from '@tanstack/react-router'
import { useState } from 'react'
import { useTranslation } from 'react-i18next'

import { MemoryBrowser } from '@/features/memory/MemoryBrowser'
import { MemoryInbox } from '@/features/memory/MemoryInbox'
import { MemoryRecallTracePanel } from '@/features/memory/MemoryRecallTracePanel'
import { MemorySettings } from '@/features/memory/MemorySettings'

type MemoryTab = 'items' | 'inbox' | 'traces' | 'settings'

function MemoryPage() {
  const { t } = useTranslation('memory')
  const [tab, setTab] = useState<MemoryTab>('items')

  const tabs: { id: MemoryTab; label: string }[] = [
    { id: 'items', label: t('items') },
    { id: 'inbox', label: t('inbox') },
    { id: 'traces', label: t('recallTraces') },
    { id: 'settings', label: t('settings') },
  ]

  return (
    <div>
      <div className="flex border-b">
        {tabs.map((t) => (
          <button
            key={t.id}
            onClick={() => setTab(t.id)}
            className={`px-4 py-2 text-sm font-medium border-b-2 transition-colors ${
              tab === t.id
                ? 'border-primary text-primary'
                : 'border-transparent text-muted-foreground hover:text-foreground'
            }`}
          >
            {t.label}
          </button>
        ))}
      </div>
      <div>
        {tab === 'items' && <MemoryBrowser />}
        {tab === 'inbox' && <MemoryInbox />}
        {tab === 'traces' && <MemoryRecallTracePanel />}
        {tab === 'settings' && <MemorySettings />}
      </div>
    </div>
  )
}

export const Route = createLazyFileRoute('/memory')({
  component: MemoryPage,
})
