import type { HTMLAttributes, LabelHTMLAttributes, ReactNode } from 'react'

import { cn } from '@/shared/lib/utils'

export function Field({ className, ...props }: HTMLAttributes<HTMLDivElement>) {
  return <div className={cn('space-y-2', className)} data-slot="field" {...props} />
}

type FieldLabelProps = LabelHTMLAttributes<HTMLLabelElement> & {
  children: ReactNode
  htmlFor: string
}

export function FieldLabel({ children, className, htmlFor, ...props }: FieldLabelProps) {
  return (
    <label
      className={cn('block font-medium text-sm tracking-normal', className)}
      data-slot="field-label"
      htmlFor={htmlFor}
      {...props}
    >
      {children}
    </label>
  )
}

export function FieldDescription({ className, ...props }: HTMLAttributes<HTMLParagraphElement>) {
  return <p className={cn('text-body-muted', className)} data-slot="field-description" {...props} />
}

export function FieldError({ className, ...props }: HTMLAttributes<HTMLParagraphElement>) {
  return (
    <p className={cn('text-destructive text-sm', className)} data-slot="field-error" {...props} />
  )
}

export function FieldControl({
  children,
  className,
  fieldId,
  label,
}: {
  children: ReactNode
  className?: string
  fieldId: string
  label: string
}) {
  return (
    <Field className={className}>
      <FieldLabel htmlFor={fieldId}>{label}</FieldLabel>
      {children}
    </Field>
  )
}
