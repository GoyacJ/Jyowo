import { AlertTriangle } from 'lucide-react'

function getErrorMessage(error: unknown): string {
  return error instanceof Error ? error.message : String(error)
}

export function RouteErrorMessage({ error }: { error: unknown }) {
  return (
    <section className="mx-auto flex w-full max-w-3xl flex-col gap-3 rounded-lg border border-destructive/30 bg-destructive/5 p-6 text-destructive">
      <div className="flex items-center gap-2 font-semibold">
        <AlertTriangle data-icon="inline-start" />
        Route error
      </div>
      <p className="text-sm text-destructive">{getErrorMessage(error)}</p>
    </section>
  )
}
