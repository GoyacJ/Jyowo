import { forwardRef, type TextareaHTMLAttributes } from 'react'

import { cn } from '@/shared/lib/utils'

export const Textarea = forwardRef<
  HTMLTextAreaElement,
  TextareaHTMLAttributes<HTMLTextAreaElement>
>(function Textarea({ className, ...props }, ref) {
  return (
    <textarea
      className={cn(
        'min-h-24 w-full resize-y rounded-md border border-input bg-background px-3 py-2 text-sm tracking-normal outline-none transition-[border-color,box-shadow] duration-200 placeholder:text-muted-foreground focus:border-ring/60 focus:ring-2 focus:ring-ring/10 disabled:cursor-not-allowed disabled:opacity-50',
        className,
      )}
      data-slot="textarea"
      ref={ref}
      {...props}
    />
  )
})
