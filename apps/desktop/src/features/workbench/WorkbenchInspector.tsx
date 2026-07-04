import { useQuery, useQueryClient } from '@tanstack/react-query'
import { X } from 'lucide-react'
import { useTranslation } from 'react-i18next'
import { ChangeSetSummary } from '@/features/conversation/evidence/ChangeSetSummary'
import { CommandExecutionView } from '@/features/conversation/evidence/CommandExecutionView'
import { DecisionPanel } from '@/features/conversation/evidence/DecisionPanel'
import { DiffPane } from '@/features/conversation/evidence/DiffPane'
import { ToolInvocationCard } from '@/features/conversation/evidence/ToolInvocationCard'
import { ArtifactImagePreview } from '@/features/conversation/timeline/artifact-segment-view'
import { useUiStore } from '@/shared/state/ui-store'
import type { WorkbenchSelection } from '@/shared/state/workbench-selection'
import type {
  ArtifactSegment,
  ChangeSet,
  CommandExecution,
  ConversationEventRef,
  ConversationTurn,
  DecisionRequestState,
  PageConversationWorktreeResponse,
  ProcessStep,
  ToolAttempt,
} from '@/shared/tauri/commands'
import { useCommandClient } from '@/shared/tauri/react'
import { Button } from '@/shared/ui/button'

type InspectorItem =
  | { kind: 'decision'; decision: DecisionRequestState }
  | { kind: 'tool'; attempt: ToolAttempt }
  | { kind: 'command'; command: CommandExecution }
  | { kind: 'diff'; changeSet: ChangeSet }
  | { kind: 'artifact'; segment: ArtifactSegment }

type InspectorPaneRendererProps = {
  selection: WorkbenchSelection
}

function InspectorPaneRenderer({ selection }: InspectorPaneRendererProps) {
  const { t } = useTranslation('conversation')
  const commandClient = useCommandClient()
  const queryClient = useQueryClient()
  const needsProjection = selection.kind !== 'context'
  const conversationId = needsProjection ? selection.conversationId : undefined
  const cachedItem =
    selection.kind === 'context' ? null : findCachedInspectorItem(queryClient, selection)

  const worktreeQuery = useQuery({
    enabled: needsProjection && conversationId !== undefined && cachedItem === null,
    queryKey: ['workbench-inspector-worktree', conversationId],
    queryFn: () =>
      commandClient.pageConversationWorktree({
        conversationId: conversationId ?? '',
        direction: 'after',
        limit: 100,
      }),
  })

  if (selection.kind === 'context') {
    return (
      <InspectorState
        description={t('inspector.contextDescription', 'Workspace context and runtime state.')}
        title={t('inspector.context', 'Context')}
      />
    )
  }

  if (cachedItem) {
    return <InspectorItemView conversationId={selection.conversationId} item={cachedItem} />
  }

  if (worktreeQuery.isPending) {
    return (
      <InspectorState
        description={t('inspector.loadingDescription', 'Fetching the selected evidence.')}
        title={t('inspector.loading', 'Loading inspector data')}
      />
    )
  }

  if (worktreeQuery.isError) {
    return (
      <InspectorState
        description={t('inspector.errorDescription', 'The selected evidence could not be loaded.')}
        title={t('inspector.error', 'Inspector data failed to load')}
      />
    )
  }

  const item = findInspectorItem(selection, worktreeQuery.data.turns)

  if (!item) {
    return (
      <InspectorState
        description={t(
          'inspector.notFoundDescription',
          'The selected item is not present in the current worktree projection.',
        )}
        title={t('inspector.notFound', 'Selection unavailable')}
      />
    )
  }

  return <InspectorItemView conversationId={selection.conversationId} item={item} />
}

function InspectorItemView({
  conversationId,
  item,
}: {
  conversationId: string
  item: InspectorItem
}) {
  const { t } = useTranslation('conversation')

  switch (item.kind) {
    case 'decision':
      return (
        <div className="grid gap-3 p-3">
          <DecisionPanel conversationId={conversationId} decision={item.decision} />
          {item.decision.decisionOptions.length > 0 ? (
            <section className="grid gap-2 rounded-md border border-border px-3 py-2">
              <h4 className="text-muted-foreground text-xs font-medium">
                {t('inspector.decisionOptions', 'Decision options')}
              </h4>
              {item.decision.decisionOptions.map((option) => (
                <div className="text-sm" key={option.id}>
                  {option.label}
                </div>
              ))}
            </section>
          ) : null}
        </div>
      )
    case 'tool':
      return (
        <div className="grid gap-3 p-3">
          <ToolInvocationCard attempt={item.attempt} />
          {item.attempt.permission ? (
            <DecisionPanel conversationId={conversationId} decision={item.attempt.permission} />
          ) : null}
          {item.attempt.failureSummary ? (
            <p className="border-destructive/40 border-l pl-3 text-destructive text-sm">
              {item.attempt.failureSummary}
            </p>
          ) : null}
        </div>
      )
    case 'command':
      return (
        <div className="p-3">
          <CommandExecutionView command={item.command} conversationId={conversationId} />
        </div>
      )
    case 'diff':
      return (
        <div className="flex h-full min-h-0 flex-col gap-3 p-3">
          <ChangeSetSummary changeSet={item.changeSet} />
          <div className="min-h-[320px] flex-1 overflow-hidden rounded-md border border-border">
            <DiffPane conversationId={conversationId} files={item.changeSet.files} />
          </div>
        </div>
      )
    case 'artifact':
      return <ArtifactInspectorPane conversationId={conversationId} segment={item.segment} />
  }
}

function findCachedInspectorItem(
  queryClient: ReturnType<typeof useQueryClient>,
  selection: Exclude<WorkbenchSelection, { kind: 'context' }>,
) {
  const cachedPages = queryClient.getQueriesData<PageConversationWorktreeResponse>({
    queryKey: ['conversation-worktree'],
  })

  for (const [, page] of cachedPages) {
    if (!page?.turns.some((turn) => turn.conversationId === selection.conversationId)) {
      continue
    }

    const item = findInspectorItem(selection, page.turns)
    if (item) {
      return item
    }
  }

  return null
}

function ArtifactInspectorPane({
  conversationId,
  segment,
}: {
  conversationId: string
  segment: ArtifactSegment
}) {
  const { t } = useTranslation('conversation')
  const commandClient = useCommandClient()
  const revision = segment.revision
  const contentQuery = useQuery({
    enabled: revision.contentRef !== undefined,
    queryKey: ['workbench-inspector-artifact-content', conversationId, revision.contentRef],
    queryFn: () =>
      commandClient.getArtifactRevisionContent({
        conversationId,
        contentRef: revision.contentRef ?? '',
      }),
  })

  return (
    <div className="grid gap-3 p-3">
      <section className="grid gap-1 rounded-md border border-border px-3 py-2">
        <div className="font-medium text-sm">{segment.title}</div>
        {segment.summary ? (
          <p className="text-muted-foreground text-sm">{segment.summary}</p>
        ) : null}
        <p className="text-muted-foreground text-xs">
          {revision.kind} · {revision.status} · {revision.revisionId}
        </p>
      </section>

      {revision.media?.kind === 'image' ? (
        <ArtifactImagePreview
          artifactId={segment.artifactId}
          conversationId={conversationId}
          title={segment.title}
        />
      ) : null}

      {revision.contentRef ? (
        contentQuery.isPending ? (
          <InspectorState
            description={t('inspector.artifactLoadingDescription', 'Fetching artifact content.')}
            title={t('inspector.artifactLoading', 'Loading artifact content')}
          />
        ) : contentQuery.isError ? (
          <InspectorState
            description={t(
              'inspector.artifactErrorDescription',
              'The artifact content could not be loaded.',
            )}
            title={t('inspector.artifactError', 'Artifact content failed to load')}
          />
        ) : (
          <div className="overflow-hidden rounded-md border border-border bg-code-background">
            <pre className="max-h-[520px] overflow-auto p-3 whitespace-pre-wrap font-mono text-xs leading-5">
              <code>{contentQuery.data.content}</code>
            </pre>
            {contentQuery.data.truncated ? (
              <div className="border-border border-t px-3 py-2 text-muted-foreground text-xs">
                {t('inspector.artifactContentTruncated', 'Artifact content page truncated')}
              </div>
            ) : null}
          </div>
        )
      ) : (
        <InspectorState
          description={t(
            'inspector.artifactNoContentDescription',
            'This artifact revision does not expose a content reference.',
          )}
          title={t('inspector.artifactNoContent', 'Artifact content unavailable')}
        />
      )}
    </div>
  )
}

function findInspectorItem(
  selection: Exclude<WorkbenchSelection, { kind: 'context' }>,
  turns: ConversationTurn[],
): InspectorItem | null {
  for (const turn of turns) {
    for (const segment of turn.assistant?.segments ?? []) {
      switch (segment.kind) {
        case 'toolGroup': {
          for (const attempt of segment.attempts) {
            if (selection.kind === 'tool' && attempt.toolUseId === selection.toolUseId) {
              return { kind: 'tool', attempt }
            }
            if (
              selection.kind === 'decision' &&
              attempt.permission?.requestId === selection.requestId
            ) {
              return { kind: 'decision', decision: attempt.permission }
            }
          }
          break
        }
        case 'process': {
          for (const step of segment.steps ?? []) {
            const item = findProcessStepItem(selection, step)
            if (item) {
              return item
            }
          }
          break
        }
        case 'artifact':
          if (
            selection.kind === 'artifact' &&
            segment.artifactId === selection.artifactId &&
            (!selection.revisionId || segment.revision.revisionId === selection.revisionId)
          ) {
            return { kind: 'artifact', segment }
          }
          break
        default:
          break
      }
    }
  }

  return null
}

function findProcessStepItem(
  selection: Exclude<WorkbenchSelection, { kind: 'context' }>,
  step: ProcessStep,
): InspectorItem | null {
  const detail = step.detail
  if (!detail) {
    return null
  }

  if (
    selection.kind === 'command' &&
    detail.type === 'command' &&
    commandMatchesSelection(selection, step)
  ) {
    return { kind: 'command', command: detail }
  }

  if (selection.kind === 'diff' && detail.type === 'diff' && detail.id === selection.changeSetId) {
    return {
      kind: 'diff',
      changeSet: {
        id: detail.id,
        summary: detail.summary,
        files: detail.files,
      },
    }
  }

  return null
}

function commandMatchesSelection(
  selection: Extract<WorkbenchSelection, { kind: 'command' }>,
  step: ProcessStep,
) {
  if (step.detail?.type !== 'command') {
    return false
  }

  if (selection.fullOutputRef) {
    return step.detail.fullOutputRef === selection.fullOutputRef
  }

  if (selection.eventRef) {
    const selectedEventRef = selection.eventRef
    return step.eventRefs?.some((eventRef) => eventRefMatches(eventRef, selectedEventRef)) ?? false
  }

  return step.kind === 'command'
}

function eventRefMatches(left: ConversationEventRef, right: ConversationEventRef) {
  return (
    left.eventId === right.eventId ||
    left.cursor.eventId === right.cursor.eventId ||
    left.cursor.conversationSequence === right.cursor.conversationSequence
  )
}

function InspectorState({ title, description }: { title: string; description: string }) {
  return (
    <div className="flex h-full flex-col items-center justify-center gap-3 px-6 text-center">
      <h3 className="text-sm font-medium text-foreground">{title}</h3>
      <p className="text-xs text-muted-foreground">{description}</p>
    </div>
  )
}

export function WorkbenchInspector() {
  const { t } = useTranslation('conversation')
  const selection = useUiStore((state) => state.workbenchSelection)
  const inspectorOpen = useUiStore((state) => state.inspectorOpen)
  const setInspectorOpen = useUiStore((state) => state.setInspectorOpen)

  if (!inspectorOpen) {
    return null
  }

  return (
    <aside
      aria-label={t('inspector.label', 'Inspector')}
      className="flex h-full flex-col border-border border-l bg-background"
      style={{ width: '360px', minWidth: '280px' }}
    >
      <div className="flex h-10 items-center justify-between border-border border-b px-3">
        <span className="text-muted-foreground text-xs font-medium">
          {t('inspector.title', 'Inspector')}
        </span>
        <Button
          aria-label={t('actions.closeInspector', 'Close inspector')}
          className="size-7"
          onClick={() => setInspectorOpen(false)}
          size="icon"
          type="button"
          variant="ghost"
        >
          <X className="size-4" />
        </Button>
      </div>
      <div className="min-h-0 flex-1 overflow-auto">
        {selection ? (
          <InspectorPaneRenderer selection={selection} />
        ) : (
          <InspectorState
            description={t(
              'inspector.emptyDescription',
              'Select evidence, a decision, a diff, or an artifact to inspect it here.',
            )}
            title={t('inspector.empty', 'No Selection')}
          />
        )}
      </div>
    </aside>
  )
}
