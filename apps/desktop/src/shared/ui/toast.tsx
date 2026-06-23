import { AlertCircle, CheckCircle2, X } from 'lucide-react'
import type { ReactNode } from 'react'
import { useEffect } from 'react'

import { cn } from '@/shared/lib/utils'

type ToastVariant = 'success' | 'destructive'

type ToastProps = {
  autoCloseMs?: number
  children?: ReactNode
  closeLabel?: string
  description?: ReactNode
  onClose?: () => void
  title: ReactNode
  variant?: ToastVariant
}

export function Toast({
  autoCloseMs = 4000,
  children,
  closeLabel = 'Close notification',
  description,
  onClose,
  title,
  variant = 'success',
}: ToastProps) {
  const Icon = variant === 'success' ? CheckCircle2 : AlertCircle

  useEffect(() => {
    if (!onClose || autoCloseMs <= 0) {
      return
    }

    const timeoutId = window.setTimeout(onClose, autoCloseMs)
    return () => window.clearTimeout(timeoutId)
  }, [autoCloseMs, onClose])

  return (
    <div
      className={cn(
        'fixed top-4 right-4 z-50 flex w-[min(calc(100vw-2rem),22rem)] items-start gap-3 rounded-md border bg-surface p-3 text-sm shadow-lg',
        variant === 'success'
          ? 'border-success/30 text-foreground'
          : 'border-destructive/30 text-foreground',
      )}
      role={variant === 'success' ? 'status' : 'alert'}
    >
      <Icon
        className={cn(
          'mt-0.5 size-4 shrink-0',
          variant === 'success' ? 'text-success' : 'text-destructive',
        )}
      />
      <div className="min-w-0 flex-1">
        <div className="font-medium">{title}</div>
        {description ? <div className="mt-1 text-muted-foreground">{description}</div> : null}
        {children}
      </div>
      {onClose ? (
        <button
          aria-label={closeLabel}
          className="rounded-sm p-0.5 text-muted-foreground hover:bg-muted hover:text-foreground"
          onClick={onClose}
          type="button"
        >
          <X className="size-3.5" />
        </button>
      ) : null}
    </div>
  )
}
