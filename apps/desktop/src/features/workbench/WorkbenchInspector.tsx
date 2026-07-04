import { X } from 'lucide-react'
import { useTranslation } from 'react-i18next'
import { useUiStore } from '@/shared/state/ui-store'
import type { WorkbenchSelection } from '@/shared/state/workbench-selection'
import { Button } from '@/shared/ui/button'

type InspectorPaneRendererProps = {
  selection: WorkbenchSelection
}

function InspectorPaneRenderer({ selection }: InspectorPaneRendererProps) {
  const { t } = useTranslation('conversation')

  switch (selection.kind) {
    case 'context':
      return (
        <PanePlaceholder
          title={t('inspector.context', 'Context')}
          description={t('inspector.contextDescription', 'Workspace context and runtime state.')}
        />
      )
    case 'decision':
      return (
        <PanePlaceholder
          title={t('inspector.decision', 'Decision')}
          description={t(
            'inspector.decisionDescription',
            `Permission decision for request ${selection.requestId}.`,
          )}
        />
      )
    case 'tool':
      return (
        <PanePlaceholder
          title={t('inspector.tool', 'Tool')}
          description={t('inspector.toolDescription', `Tool invocation details.`)}
        />
      )
    case 'command':
      return (
        <PanePlaceholder
          title={t('inspector.terminal', 'Terminal')}
          description={t('inspector.terminalDescription', 'Command execution output and details.')}
        />
      )
    case 'diff':
      return (
        <PanePlaceholder
          title={t('inspector.diff', 'Diff')}
          description={t('inspector.diffDescription', 'File changes and patch details.')}
        />
      )
    case 'artifact':
      return (
        <PanePlaceholder
          title={t('inspector.artifact', 'Artifact')}
          description={t(
            'inspector.artifactDescription',
            `Artifact ${selection.artifactId} revision details.`,
          )}
        />
      )
  }
}

function PanePlaceholder({ title, description }: { title: string; description: string }) {
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
      className="flex h-full flex-col border-l border-border bg-background"
      style={{ width: '360px', minWidth: '280px' }}
    >
      <div className="flex h-10 items-center justify-between border-b border-border px-3">
        <span className="text-xs font-medium text-muted-foreground">
          {t('inspector.title', 'Inspector')}
        </span>
        <Button
          aria-label={t('actions.closeInspector')}
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
          <PanePlaceholder
            title={t('inspector.empty', 'No Selection')}
            description={t(
              'inspector.emptyDescription',
              'Select evidence, a decision, a diff, or an artifact to inspect it here.',
            )}
          />
        )}
      </div>
    </aside>
  )
}
