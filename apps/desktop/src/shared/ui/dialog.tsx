import * as DialogPrimitive from '@radix-ui/react-dialog'
import { X } from 'lucide-react'
import type { ComponentProps } from 'react'
import { useTranslation } from 'react-i18next'

import { cn } from '@/shared/lib/utils'

export const Dialog = DialogPrimitive.Root
export const DialogTrigger = DialogPrimitive.Trigger
export const DialogClose = DialogPrimitive.Close

function DialogOverlay({ className, ...props }: ComponentProps<typeof DialogPrimitive.Overlay>) {
  return (
    <DialogPrimitive.Overlay
      className={cn(
        'fixed inset-0 z-50 bg-background/45 backdrop-blur-md transition-[opacity,backdrop-filter] duration-300',
        className,
      )}
      {...props}
    />
  )
}

export function DialogContent({
  children,
  className,
  ...props
}: ComponentProps<typeof DialogPrimitive.Content>) {
  const { t } = useTranslation('common')

  return (
    <DialogPrimitive.Portal>
      <DialogOverlay />
      <DialogPrimitive.Content
        className={cn(
          'fixed top-1/2 left-1/2 z-50 grid w-[min(calc(100vw-2rem),32rem)] -translate-x-1/2 -translate-y-1/2 gap-4 rounded-md border border-border bg-popover p-6 text-popover-foreground shadow-lg outline-none animate-dialog-enter',
          className,
        )}
        {...props}
      >
        {children}
        <DialogPrimitive.Close className="absolute top-4 right-4 rounded-md p-1 opacity-70 outline-none transition-[background-color,opacity] duration-200 hover:bg-muted hover:opacity-100 focus-visible:ring-2 focus-visible:ring-ring disabled:pointer-events-none">
          <X aria-hidden="true" className="size-4" data-icon />
          <span className="sr-only">{t('close')}</span>
        </DialogPrimitive.Close>
      </DialogPrimitive.Content>
    </DialogPrimitive.Portal>
  )
}

export function DialogHeader({ className, ...props }: ComponentProps<'div'>) {
  return <div className={cn('flex flex-col gap-1.5 text-left', className)} {...props} />
}

export function DialogFooter({ className, ...props }: ComponentProps<'div'>) {
  return (
    <div
      className={cn('flex flex-col-reverse gap-2 sm:flex-row sm:justify-end', className)}
      {...props}
    />
  )
}

export function DialogTitle({ className, ...props }: ComponentProps<typeof DialogPrimitive.Title>) {
  return (
    <DialogPrimitive.Title
      className={cn('font-semibold text-foreground text-lg tracking-normal', className)}
      {...props}
    />
  )
}

export function DialogDescription({
  className,
  ...props
}: ComponentProps<typeof DialogPrimitive.Description>) {
  return (
    <DialogPrimitive.Description
      className={cn('text-muted-foreground text-sm leading-6', className)}
      {...props}
    />
  )
}
