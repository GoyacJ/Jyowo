import { useCallback, useRef } from 'react'
import { useTranslation } from 'react-i18next'

type ComposerEditorProps = {
  disabled: boolean
  value: string
  onChange: (value: string) => void
  onSubmit: () => void
}

const minHeight = 64
const maxHeight = 160

export function ComposerEditor({ disabled, value, onChange, onSubmit }: ComposerEditorProps) {
  const { t } = useTranslation('conversation')
  const ref = useRef<HTMLTextAreaElement>(null)

  const adjustHeight = useCallback(() => {
    const el = ref.current
    if (!el) return
    el.style.height = '0'
    const scroll = el.scrollHeight
    el.style.height = `${Math.min(Math.max(scroll, minHeight), maxHeight)}px`
  }, [])

  const handleChange = (event: React.ChangeEvent<HTMLTextAreaElement>) => {
    onChange(event.target.value)
    adjustHeight()
  }

  const handleKeyDown = (event: React.KeyboardEvent<HTMLTextAreaElement>) => {
    // Enter submits, Shift+Enter inserts newline
    if (event.key === 'Enter' && !event.shiftKey && !event.nativeEvent.isComposing) {
      event.preventDefault()
      onSubmit()
    }
  }

  return (
    <textarea
      ref={ref}
      aria-label={t('composer.inputLabel')}
      className="w-full resize-none bg-transparent text-sm outline-none placeholder:text-muted-foreground disabled:cursor-not-allowed disabled:opacity-60"
      disabled={disabled}
      onChange={handleChange}
      onKeyDown={handleKeyDown}
      placeholder={t('composer.placeholder')}
      rows={1}
      style={{ minHeight: `${minHeight}px`, maxHeight: `${maxHeight}px` }}
      value={value}
    />
  )
}
