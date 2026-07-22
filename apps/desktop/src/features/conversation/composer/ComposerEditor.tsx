import { forwardRef, useCallback, useRef } from 'react'
import { useTranslation } from 'react-i18next'

import { Textarea } from '@/shared/ui/textarea'

type ComposerEditorProps = {
  activeDescendant?: string
  controls?: string
  disabled: boolean
  expanded: boolean
  value: string
  onChange: (value: string, cursorPosition: number) => void
  onCursorChange?: (cursorPosition: number) => void
  onKeyCommand?: (event: React.KeyboardEvent<HTMLTextAreaElement>) => boolean
  onSubmit: () => void
}

const minHeight = 44
const maxHeight = 160

export const ComposerEditor = forwardRef<HTMLTextAreaElement, ComposerEditorProps>(
  function ComposerEditor(
    {
      activeDescendant,
      controls,
      disabled,
      expanded,
      value,
      onChange,
      onCursorChange,
      onKeyCommand,
      onSubmit,
    },
    forwardedRef,
  ) {
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
      onChange(event.target.value, event.target.selectionStart ?? event.target.value.length)
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
        ref={(element) => {
          ref.current = element
          if (typeof forwardedRef === 'function') {
            forwardedRef(element)
          } else if (forwardedRef) {
            forwardedRef.current = element
          }
        }}
        aria-activedescendant={activeDescendant}
        aria-autocomplete="list"
        aria-controls={controls}
        aria-expanded={expanded}
        aria-label={t('composer.inputLabel')}
        className="min-h-0 w-full resize-none border-0 bg-transparent px-0 py-0 shadow-none focus:border-transparent focus:ring-0"
        disabled={disabled}
        onChange={handleChange}
        onKeyDown={handleKeyDown}
        onSelect={(event) =>
          onCursorChange?.(event.currentTarget.selectionStart ?? event.currentTarget.value.length)
        }
        placeholder={t('composer.placeholder')}
        rows={1}
        style={{ minHeight: `${minHeight}px`, maxHeight: `${maxHeight}px` }}
        value={value}
      />
    )
  },
)
