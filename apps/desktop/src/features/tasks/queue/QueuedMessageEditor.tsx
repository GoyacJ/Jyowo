import { useState } from 'react'
import { useTranslation } from 'react-i18next'

export function QueuedMessageEditor({
  initialValue,
  label,
  onCancel,
  onSave,
}: {
  initialValue: string
  label: string
  onCancel: () => void
  onSave: (content: string) => Promise<void>
}) {
  const { t } = useTranslation('tasks')
  const [content, setContent] = useState(initialValue)
  const [saving, setSaving] = useState(false)

  async function save() {
    const nextContent = content.trim()
    if (!nextContent || saving) return
    setSaving(true)
    try {
      await onSave(nextContent)
    } catch {
      // The queue-level live region owns the error message; keep this draft open for retry.
    } finally {
      setSaving(false)
    }
  }

  return (
    <div className="mt-2">
      <textarea
        aria-label={label}
        className="min-h-20 w-full resize-y rounded-md border border-border bg-background px-3 py-2 text-sm outline-none focus-visible:border-ring focus-visible:ring-2 focus-visible:ring-ring/20"
        disabled={saving}
        onChange={(event) => setContent(event.target.value)}
        value={content}
      />
      <div className="mt-2 flex justify-end gap-2">
        <button
          className="rounded-md px-2.5 py-1.5 text-muted-foreground text-xs hover:bg-muted hover:text-foreground disabled:opacity-50"
          disabled={saving}
          onClick={onCancel}
          type="button"
        >
          {t('queue.cancelEdit')}
        </button>
        <button
          className="rounded-md bg-primary px-2.5 py-1.5 font-medium text-primary-foreground text-xs disabled:opacity-50"
          disabled={saving || content.trim().length === 0}
          onClick={() => void save()}
          type="button"
        >
          {t('queue.saveEdit')}
        </button>
      </div>
    </div>
  )
}
