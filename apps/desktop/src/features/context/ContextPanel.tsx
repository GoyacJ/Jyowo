import { ChevronDown, X } from 'lucide-react'

import { ContextSection } from './ContextSection'
import { type ContextFileReference, FileReferenceList } from './FileReferenceList'
import { NextActionList } from './NextActionList'

type ContextDecision = {
  detail: string
  title: string
}

export type WorkspaceContext = {
  activeArtifact?: string
  decisions: ContextDecision[]
  files: ContextFileReference[]
  nextActions: string[]
  path: string
  project: string
  totalFileCount?: number
}

type ContextPanelProps = {
  context: WorkspaceContext | null
  errorMessage?: string
  loading?: boolean
  onAddFile?: () => void
  onClose?: () => void
  onDecisionSelect?: (decision: ContextDecision) => void
  onNextAction?: (action: string) => void
  onShowAllFiles?: () => void
}

export function ContextPanel({
  context,
  errorMessage,
  loading = false,
  onAddFile,
  onClose,
  onDecisionSelect,
  onNextAction,
  onShowAllFiles,
}: ContextPanelProps) {
  return (
    <aside
      aria-label="Context"
      className="flex min-h-0 flex-col border-border border-l bg-background"
    >
      <div className="flex h-16 items-center justify-between px-8">
        <div className="font-semibold">Context</div>
        {onClose ? (
          <button
            aria-label="Close context"
            className="rounded-md p-1.5 text-muted-foreground hover:bg-muted hover:text-foreground"
            onClick={onClose}
            type="button"
          >
            <X className="size-4" />
          </button>
        ) : null}
      </div>
      {loading ? (
        <LoadingContextPanel />
      ) : errorMessage ? (
        <ErrorContextPanel message={errorMessage} />
      ) : context ? (
        <ContextPanelContent
          context={context}
          onAddFile={onAddFile}
          onDecisionSelect={onDecisionSelect}
          onNextAction={onNextAction}
          onShowAllFiles={onShowAllFiles}
        />
      ) : (
        <EmptyContextPanel />
      )}
    </aside>
  )
}

function LoadingContextPanel() {
  return <div className="px-8 text-muted-foreground text-sm">Loading context</div>
}

function ErrorContextPanel({ message }: { message: string }) {
  return (
    <div className="px-8 text-sm">
      <div className="rounded-md border border-destructive/30 bg-destructive/5 p-4">
        <div className="font-medium text-destructive">Context unavailable</div>
        <p className="mt-2 text-destructive/80">{message}</p>
      </div>
    </div>
  )
}

function ContextPanelContent({
  context,
  onAddFile,
  onDecisionSelect,
  onNextAction,
  onShowAllFiles,
}: {
  context: WorkspaceContext
  onAddFile?: () => void
  onDecisionSelect?: (decision: ContextDecision) => void
  onNextAction?: (action: string) => void
  onShowAllFiles?: () => void
}) {
  const totalFileCount = context.totalFileCount ?? context.files.length
  const hiddenFileCount = Math.max(totalFileCount - context.files.length, 0)

  return (
    <div className="min-h-0 flex-1 space-y-6 overflow-y-auto px-8 pb-6 text-sm">
      <section className="space-y-3">
        <div className="text-muted-foreground">Project</div>
        <div className="font-medium">{context.project}</div>
        <div className="text-muted-foreground">Path</div>
        <div className="break-all font-mono text-xs">{context.path}</div>
      </section>

      <ContextSection
        action={
          onAddFile ? (
            <button
              aria-label="Add file"
              className="text-lg leading-none"
              onClick={onAddFile}
              type="button"
            >
              +
            </button>
          ) : null
        }
        title="Files"
      >
        <FileReferenceList files={context.files} />
        {hiddenFileCount > 0 && onShowAllFiles ? (
          <button
            aria-label="Show all files"
            className="mt-3 text-muted-foreground text-xs hover:text-foreground"
            onClick={onShowAllFiles}
            type="button"
          >
            Show all ({totalFileCount})
          </button>
        ) : null}
      </ContextSection>

      <ContextSection title="Active artifact">
        <div className="font-medium">{context.activeArtifact ?? 'No active artifact'}</div>
        {context.activeArtifact ? <ActiveArtifactThumbnail title={context.activeArtifact} /> : null}
      </ContextSection>

      <ContextSection title="Decisions needed">
        {context.decisions.length > 0 ? (
          <div className="space-y-2">
            {context.decisions.map((decision) =>
              onDecisionSelect ? (
                <button
                  className="flex w-full items-center justify-between rounded-md border border-border bg-surface px-3 py-2 text-left hover:bg-muted"
                  key={decision.title}
                  onClick={() => onDecisionSelect(decision)}
                  type="button"
                >
                  <DecisionContent decision={decision} interactive />
                </button>
              ) : (
                <div
                  className="rounded-md border border-border bg-surface px-3 py-2"
                  key={decision.title}
                >
                  <DecisionContent decision={decision} />
                </div>
              ),
            )}
          </div>
        ) : (
          <p className="text-muted-foreground text-sm">No decisions needed.</p>
        )}
      </ContextSection>

      <ContextSection title="Next actions">
        <NextActionList actions={context.nextActions} onNextAction={onNextAction} />
      </ContextSection>
    </div>
  )
}

function ActiveArtifactThumbnail({ title }: { title: string }) {
  return (
    <div
      aria-label={`${title} preview`}
      className="mt-3 overflow-hidden rounded-md border border-border bg-surface"
      role="img"
    >
      <div className="grid h-24 grid-cols-[52px_minmax(0,1fr)] bg-background">
        <div className="space-y-2 border-border border-r bg-muted/45 p-2">
          <span className="block h-1.5 w-6 rounded bg-muted-foreground/30" />
          <span className="block h-1.5 w-8 rounded bg-muted-foreground/20" />
          <span className="block h-1.5 w-7 rounded bg-muted-foreground/20" />
          <span className="block h-1.5 w-9 rounded bg-muted-foreground/20" />
        </div>
        <div className="space-y-2 p-3">
          <span className="block h-2 w-24 rounded bg-muted-foreground/25" />
          <span className="block h-1.5 w-32 rounded bg-muted-foreground/15" />
          <span className="block h-1.5 w-20 rounded bg-muted-foreground/15" />
        </div>
      </div>
    </div>
  )
}

function DecisionContent({
  decision,
  interactive,
}: {
  decision: ContextDecision
  interactive?: boolean
}) {
  return (
    <>
      <span className="flex min-w-0 items-start gap-2.5">
        <span aria-hidden="true" className="mt-1.5 size-1.5 shrink-0 rounded-full bg-warning" />
        <span className="min-w-0">
          <span className="block truncate">{decision.title}</span>
          <span className="block text-muted-foreground text-xs">{decision.detail}</span>
        </span>
      </span>
      {interactive ? (
        <ChevronDown className="size-4 shrink-0 -rotate-90 text-muted-foreground" />
      ) : null}
    </>
  )
}

function EmptyContextPanel() {
  return (
    <div className="px-8 text-sm">
      <div className="rounded-md border border-border bg-surface p-4">
        <div className="font-medium">No context selected</div>
        <p className="mt-2 text-muted-foreground">
          Start a conversation to attach project context.
        </p>
      </div>
    </div>
  )
}
