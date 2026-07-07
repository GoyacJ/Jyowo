import { Eye, EyeOff } from 'lucide-react'
import { useState } from 'react'
import { useTranslation } from 'react-i18next'

export function RedactedBody({ originalBody, className }: { originalBody: string; className?: string }) {
  const { t } = useTranslation('conversation')
  const [revealed, setRevealed] = useState(false)

  if (revealed) {
    return (
      <div className="grid gap-1.5">
        <p className={className}>{originalBody}</p>
        <button
          className="inline-flex items-center gap-1 self-start text-muted-foreground text-xs hover:text-foreground"
          onClick={() => setRevealed(false)}
          type="button"
        >
          <EyeOff aria-hidden="true" className="size-3" />
          {t('timeline.redactedBodyHide')}
        </button>
      </div>
    )
  }

  return (
    <div className="grid gap-1.5">
      <p className={className}>
        <span className="italic text-muted-foreground">{t('timeline.redactedBody')}</span>
      </p>
      <button
        className="inline-flex items-center gap-1 self-start text-muted-foreground text-xs hover:text-foreground"
        onClick={() => setRevealed(true)}
        type="button"
      >
        <Eye aria-hidden="true" className="size-3" />
        {t('timeline.redactedBodyShow')}
      </button>
    </div>
  )
}
