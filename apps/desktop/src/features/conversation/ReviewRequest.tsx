import { Button } from '@/shared/ui/button'

export interface ReviewRequestProps {
  continueActionLabel: string
  onContinue?: () => void
  title: string
}

export function ReviewRequest({ continueActionLabel, onContinue, title }: ReviewRequestProps) {
  return (
    <section
      aria-label="Review request"
      className="mt-4 flex items-center justify-between gap-3 rounded-md border border-border bg-surface px-4 py-3"
    >
      <span className="font-medium text-sm">{title}</span>
      <Button size="sm" type="button" variant="outline" onClick={onContinue}>
        {continueActionLabel}
      </Button>
    </section>
  )
}
