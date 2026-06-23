import ReactMarkdown, { type Components } from 'react-markdown'
import remarkGfm from 'remark-gfm'

import { cn } from '@/shared/lib/utils'

export type MarkdownMessageProps = {
  children: string
  className?: string
}

const markdownComponents: Components = {
  a({ className, ...props }) {
    return (
      <a
        className={cn('font-medium text-primary underline-offset-4 hover:underline', className)}
        rel="noreferrer"
        target="_blank"
        {...props}
      />
    )
  },
  blockquote({ className, ...props }) {
    return (
      <blockquote
        className={cn('border-border border-l-2 pl-3 text-muted-foreground', className)}
        {...props}
      />
    )
  },
  code({ className, ...props }) {
    return (
      <code
        className={cn(
          'rounded-sm bg-muted px-1 py-0.5 font-mono text-[0.92em] text-foreground',
          className,
        )}
        {...props}
      />
    )
  },
  ol({ className, ...props }) {
    return <ol className={cn('ml-5 list-decimal space-y-1', className)} {...props} />
  },
  p({ className, ...props }) {
    return <p className={cn('leading-7', className)} {...props} />
  },
  pre({ className, ...props }) {
    return (
      <pre
        className={cn(
          'overflow-x-auto rounded-md border border-border bg-muted p-3 text-sm leading-6',
          className,
        )}
        {...props}
      />
    )
  },
  table({ className, ...props }) {
    return (
      <div className="overflow-x-auto">
        <table className={cn('w-full border-collapse text-sm', className)} {...props} />
      </div>
    )
  },
  td({ className, ...props }) {
    return <td className={cn('border border-border px-3 py-2 align-top', className)} {...props} />
  },
  th({ className, ...props }) {
    return (
      <th
        className={cn('border border-border bg-muted px-3 py-2 text-left font-medium', className)}
        {...props}
      />
    )
  },
  ul({ className, ...props }) {
    return <ul className={cn('ml-5 list-disc space-y-1', className)} {...props} />
  },
}

export function MarkdownMessage({ children, className }: MarkdownMessageProps) {
  return (
    <div className={cn('space-y-3 overflow-wrap-anywhere text-sm', className)}>
      <ReactMarkdown components={markdownComponents} remarkPlugins={[remarkGfm]}>
        {children}
      </ReactMarkdown>
    </div>
  )
}
