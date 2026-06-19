type NextActionListProps = {
  actions: string[]
  onNextAction?: (action: string) => void
}

export function NextActionList({ actions, onNextAction }: NextActionListProps) {
  if (actions.length === 0) {
    return <p className="text-muted-foreground text-sm">No next actions.</p>
  }

  return (
    <ul aria-label="Next actions" className="space-y-2">
      {actions.map((action) => (
        <li key={action}>
          {onNextAction ? (
            <button
              className="w-full rounded-md border border-border bg-surface px-3 py-2 text-left text-sm hover:bg-muted"
              onClick={() => onNextAction(action)}
              type="button"
            >
              {action}
            </button>
          ) : (
            <div className="rounded-md border border-border bg-surface px-3 py-2 text-sm">
              {action}
            </div>
          )}
        </li>
      ))}
    </ul>
  )
}
