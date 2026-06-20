import { Paperclip, Send } from 'lucide-react'
import { useState } from 'react'
import { useTranslation } from 'react-i18next'

type ComposerProps = {
  onSubmit: (text: string) => void
  pending?: boolean
  disabled?: boolean
  errorMessage?: string
  onRetry?: () => void
}

export function Composer({
  onSubmit,
  pending = false,
  disabled = false,
  errorMessage,
  onRetry,
}: ComposerProps) {
  const { t } = useTranslation(['common', 'conversation'])
  const [text, setText] = useState('')
  const isDisabled = pending || disabled
  const canSubmit = text.trim().length > 0 && !isDisabled

  return (
    <form
      className="rounded-md border border-border bg-surface px-3 py-2 shadow-sm"
      onSubmit={(event) => {
        event.preventDefault()

        const submittedText = text.trim()
        if (!submittedText || isDisabled) {
          return
        }

        onSubmit(submittedText)
        setText('')
      }}
    >
      {errorMessage ? (
        <div className="mb-3 flex items-center justify-between gap-3 rounded-md border border-destructive/30 bg-destructive/5 px-3 py-2 text-sm text-destructive">
          <span>{errorMessage}</span>
          {onRetry ? (
            <button
              className="rounded-md border border-destructive/30 px-2 py-1 text-xs font-medium hover:bg-destructive/10 disabled:cursor-not-allowed disabled:opacity-60"
              disabled={isDisabled}
              onClick={onRetry}
              type="button"
            >
              {t('common:retry')}
            </button>
          ) : null}
        </div>
      ) : null}

      <textarea
        className="h-8 w-full resize-none bg-transparent text-sm outline-none placeholder:text-muted-foreground disabled:cursor-not-allowed disabled:opacity-60"
        disabled={isDisabled}
        onChange={(event) => setText(event.target.value)}
        placeholder={t('conversation:composer.placeholder')}
        rows={1}
        value={text}
      />
      <div className="mt-1 flex items-center justify-between">
        <div className="flex items-center gap-2 text-muted-foreground">
          <button
            aria-label={t('conversation:composer.attachFile')}
            className="rounded-md p-1.5 hover:bg-muted disabled:cursor-not-allowed disabled:opacity-60"
            disabled={isDisabled}
            type="button"
          >
            <Paperclip className="size-4" />
          </button>
          <button
            className="rounded-md px-2 py-1 font-mono text-sm hover:bg-muted disabled:cursor-not-allowed disabled:opacity-60"
            disabled={isDisabled}
            type="button"
          >
            @
          </button>
          <button
            className="rounded-md px-2 py-1 font-mono text-sm hover:bg-muted disabled:cursor-not-allowed disabled:opacity-60"
            disabled={isDisabled}
            type="button"
          >
            &gt;_
          </button>
        </div>
        <div className="flex items-center gap-2">
          <span className="rounded-md border border-border px-3 py-1 text-sm">
            {t('common:local')}
          </span>
          <button
            aria-label={t('conversation:composer.sendMessage')}
            className="rounded-md bg-primary p-2 text-primary-foreground disabled:cursor-not-allowed disabled:opacity-60"
            disabled={!canSubmit}
            type="submit"
          >
            <Send className="size-4" />
          </button>
        </div>
      </div>
    </form>
  )
}
