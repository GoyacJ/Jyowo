import { useCallback, useRef } from 'react'
import { useTranslation } from 'react-i18next'

import { Textarea } from '@/shared/ui/textarea'

type ComposerEditorProps = {
  disabled: boolean
  value: string
  onChange: (value: string) => void
  onKeyCommand?: (event: React.KeyboardEvent<HTMLTextAreaElement>) => boolean
  onSubmit: () => void
}

const minHeight = 64
const maxHeight = 160

export function ComposerEditor({
  disabled,
  value,
  onChange,
  onKeyCommand,
  onSubmit,
}: ComposerEditorProps) {
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
    if (onKeyCommand?.(event)) {
      event.preventDefault()
      return
    }

    // Enter submits, Shift+Enter inserts newline
    if (event.key === 'Enter' && !event.shiftKey && !event.nativeEvent.isComposing) {
      event.preventDefault()
      onSubmit()
    }
  }

  return (
    <Textarea
      ref={ref}
      aria-label={t('composer.inputLabel')}
      className="min-h-0 w-full resize-none border-0 bg-transparent px-0 py-0 shadow-none focus:border-transparent focus:ring-0"
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
