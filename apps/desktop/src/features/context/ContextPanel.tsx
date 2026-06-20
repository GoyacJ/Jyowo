import { ChevronDown, X } from 'lucide-react'
import { useTranslation } from 'react-i18next'

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
  const { t } = useTranslation('context')

  return (
    <aside
      aria-label={t('title')}
      className="flex min-h-0 flex-col border-border border-l bg-background"
    >
      <div className="flex h-14 items-center justify-between px-6">
        <div className="font-semibold">{t('title')}</div>
        {onClose ? (
          <button
            aria-label={t('close')}
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
  const { t } = useTranslation('context')

  return <div className="px-6 text-muted-foreground text-sm">{t('loading')}</div>
}

function ErrorContextPanel({ message }: { message: string }) {
  const { t } = useTranslation('context')

  return (
    <div className="px-6 text-sm">
      <div className="rounded-md border border-destructive/30 bg-destructive/5 p-4">
        <div className="font-medium text-destructive">{t('unavailable')}</div>
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
  const { t } = useTranslation('context')
  const totalFileCount = context.totalFileCount ?? context.files.length
  const hiddenFileCount = Math.max(totalFileCount - context.files.length, 0)

  return (
    <div className="min-h-0 flex-1 space-y-5 overflow-y-auto px-6 pb-5 text-sm">
      <section className="space-y-3">
        <div className="text-muted-foreground">{t('project')}</div>
        <div className="font-medium">{context.project}</div>
        <div className="text-muted-foreground">{t('path')}</div>
        <div className="break-all font-mono text-xs">{context.path}</div>
      </section>

      <ContextSection
        action={
          onAddFile ? (
            <button
              aria-label={t('addFile')}
              className="text-lg leading-none"
              onClick={onAddFile}
              type="button"
            >
              +
            </button>
          ) : null
        }
        title={t('files')}
      >
        <FileReferenceList files={context.files} />
        {hiddenFileCount > 0 && onShowAllFiles ? (
          <button
            aria-label={t('showAllFilesLabel')}
            className="mt-3 text-muted-foreground text-xs hover:text-foreground"
            onClick={onShowAllFiles}
            type="button"
          >
            {t('showAllFiles', { count: totalFileCount })}
          </button>
        ) : null}
      </ContextSection>

      <ContextSection title={t('activeArtifact')}>
        <div className="font-medium">{context.activeArtifact ?? t('noActiveArtifact')}</div>
        {context.activeArtifact ? <ActiveArtifactThumbnail title={context.activeArtifact} /> : null}
      </ContextSection>

      <ContextSection title={t('decisionsNeeded')}>
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
          <p className="text-muted-foreground text-sm">{t('noDecisionsNeeded')}</p>
        )}
      </ContextSection>

      <ContextSection title={t('nextActions')}>
        <NextActionList actions={context.nextActions} onNextAction={onNextAction} />
      </ContextSection>
    </div>
  )
}

function ActiveArtifactThumbnail({ title }: { title: string }) {
  const { t } = useTranslation('context')

  return (
    <div
      aria-label={t('preview', { title })}
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
  const { t } = useTranslation('context')

  return (
    <div className="px-6 text-sm">
      <div className="rounded-md border border-border bg-surface p-4">
        <div className="font-medium">{t('emptyTitle')}</div>
        <p className="mt-2 text-muted-foreground">{t('emptyDescription')}</p>
      </div>
    </div>
  )
}
