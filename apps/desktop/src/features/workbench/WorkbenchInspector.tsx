import { useQuery } from '@tanstack/react-query'
import { X } from 'lucide-react'
import type { ReactNode } from 'react'
import { useTranslation } from 'react-i18next'
import { ChangeSetSummary } from '@/features/conversation/evidence/ChangeSetSummary'
import { CommandExecutionView } from '@/features/conversation/evidence/CommandExecutionView'
import { DecisionPanel } from '@/features/conversation/evidence/DecisionPanel'
import { DiffPane } from '@/features/conversation/evidence/DiffPane'
import { ToolInvocationCard } from '@/features/conversation/evidence/ToolInvocationCard'
import { useUiStore } from '@/shared/state/ui-store'
import type { WorkbenchSelection } from '@/shared/state/workbench-selection'
import type {
  ConversationInspectorItem,
  ConversationInspectorSelection,
} from '@/shared/tauri/commands'
import { getCommandErrorMessage } from '@/shared/tauri/errors'
import { useCommandClient } from '@/shared/tauri/react'
import { Button } from '@/shared/ui/button'
import { ArtifactPane } from './artifacts/ArtifactPane'

type InspectorPaneRendererProps = {
  contextPane?: ReactNode
  selection: WorkbenchSelection
}

function InspectorPaneRenderer({ contextPane, selection }: InspectorPaneRendererProps) {
  const { t } = useTranslation('conversation')
  const commandClient = useCommandClient()
  const needsProjection = selection.kind !== 'context'
  const conversationId = needsProjection ? selection.conversationId : undefined
  const inspectorSelection =
    selection.kind === 'context' ? null : inspectorSelectionFromWorkbenchSelection(selection)

  const inspectorQuery = useQuery({
    enabled: needsProjection && conversationId !== undefined && inspectorSelection !== null,
    queryKey: ['workbench-inspector-item', conversationId, inspectorSelection],
    queryFn: () =>
      commandClient.getConversationInspectorItem({
        conversationId: conversationId ?? '',
        selection: inspectorSelection ?? { kind: 'turn', turnId: '' },
      }),
  })

  if (selection.kind === 'context') {
    return contextPane ? (
      contextPane
    ) : (
      <InspectorState
        description={t('inspector.contextDescription', 'Workspace context and runtime state.')}
        title={t('inspector.context', 'Context')}
      />
    )
  }

  if (inspectorQuery.isPending) {
    return (
      <InspectorState
        description={t('inspector.loadingDescription', 'Fetching the selected evidence.')}
        title={t('inspector.loading', 'Loading inspector data')}
      />
    )
  }

  if (inspectorQuery.isError) {
    return (
      <InspectorState
        action={
          <Button onClick={() => void inspectorQuery.refetch()} size="sm" type="button">
            {t('inspector.retry', 'Retry')}
          </Button>
        }
        description={getCommandErrorMessage(inspectorQuery.error)}
        title={t('inspector.error', 'Inspector data failed to load')}
      />
    )
  }

  const item = inspectorQuery.data.item

  if (item.kind === 'empty') {
    return (
      <InspectorState
        description={t(
          'inspector.notFoundDescription',
          'The selected item is not present in the worktree projection.',
        )}
        title={t('inspector.notFound', 'Selection unavailable')}
      />
    )
  }

  return (
    <InspectorItemView
      conversationId={selection.conversationId}
      item={item}
      selection={selection}
    />
  )
}

function InspectorItemView({
  conversationId,
  item,
  selection,
}: {
  conversationId: string
  item: Exclude<ConversationInspectorItem, { kind: 'empty' }>
  selection: Exclude<WorkbenchSelection, { kind: 'context' }>
}) {
  const { t } = useTranslation('conversation')

  switch (item.kind) {
    case 'turn':
      return (
        <InspectorState
          description={item.turn.user.body}
          title={t('inspector.turn', 'Conversation turn')}
        />
      )
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
          <CommandExecutionView
            allowFullOutputFetch={true}
            command={item.command}
            conversationId={conversationId}
          />
        </div>
      )
    case 'diff':
      return (
        <div className="flex h-full min-h-0 flex-col gap-3 p-3">
          <ChangeSetSummary changeSet={item.changeSet} />
          <div className="min-h-[320px] flex-1 overflow-hidden rounded-md border border-border">
            <DiffPane
              allowFullPatchFetch={true}
              conversationId={conversationId}
              files={item.changeSet.files}
            />
          </div>
        </div>
      )
    case 'artifact':
      return (
        <ArtifactPane
          conversationId={conversationId}
          initialRevisionId={selection.kind === 'artifact' ? selection.revisionId : undefined}
          segment={item.segment}
        />
      )
  }
}

function inspectorSelectionFromWorkbenchSelection(
  selection: Exclude<WorkbenchSelection, { kind: 'context' }>,
): ConversationInspectorSelection {
  switch (selection.kind) {
    case 'decision':
      return { kind: 'decision', requestId: selection.requestId }
    case 'tool':
      return { kind: 'tool', toolUseId: selection.toolUseId }
    case 'command':
      return {
        kind: 'command',
        fullOutputRef: selection.fullOutputRef,
        eventId: selection.eventRef?.eventId,
      }
    case 'diff':
      return { kind: 'diff', changeSetId: selection.changeSetId }
    case 'artifact':
      return selection.revisionId
        ? {
            kind: 'artifactRevision',
            artifactId: selection.artifactId,
            revisionId: selection.revisionId,
          }
        : { kind: 'artifact', artifactId: selection.artifactId }
  }
}

function InspectorState({
  action,
  title,
  description,
}: {
  action?: ReactNode
  title: string
  description: string
}) {
  return (
    <div className="flex h-full flex-col items-center justify-center gap-3 px-6 text-center">
      <h3 className="text-sm font-medium text-foreground">{title}</h3>
      <p className="text-xs text-muted-foreground">{description}</p>
      {action}
    </div>
  )
}

export function WorkbenchInspector({ contextPane }: { contextPane?: ReactNode } = {}) {
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
      data-workbench-scope="conversation"
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
          <InspectorPaneRenderer contextPane={contextPane} selection={selection} />
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
