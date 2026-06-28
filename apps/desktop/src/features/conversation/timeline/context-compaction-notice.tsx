export function ContextCompactionNotice({ body }: { body: string }) {
  return (
    <div className="flex items-center gap-3 text-muted-foreground text-xs">
      <span className="h-px min-w-8 flex-1 bg-border" />
      <span>{body}</span>
      <span className="h-px min-w-8 flex-1 bg-border" />
    </div>
  )
}
