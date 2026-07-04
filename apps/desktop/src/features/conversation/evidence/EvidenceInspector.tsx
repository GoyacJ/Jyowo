import { useTranslation } from 'react-i18next'
import type { WorkbenchSelection } from '@/shared/state/workbench-selection'

export function EvidenceInspector({ selection }: { selection: WorkbenchSelection }) {
  const { t } = useTranslation('conversation')

  if (selection.kind === 'command') {
    return (
      <div className="p-4">
        <h3 className="mb-2 font-medium text-sm">{t('inspector.terminal', 'Terminal')}</h3>
        <p className="text-muted-foreground text-xs">
          {t(
            'inspector.terminalDetail',
            'Select a command step in the timeline to view full output here.',
          )}
        </p>
        {selection.fullOutputRef ? (
          <p className="mt-2 font-mono text-muted-foreground text-xs">
            ref: {selection.fullOutputRef}
          </p>
        ) : null}
      </div>
    )
  }

  if (selection.kind === 'tool') {
    return (
      <div className="p-4">
        <h3 className="mb-2 font-medium text-sm">{t('inspector.tool', 'Tool')}</h3>
        <p className="text-muted-foreground text-xs">
          {t(
            'inspector.toolDetail',
            'Select a tool invocation in the timeline to view full details here.',
          )}
        </p>
      </div>
    )
  }

  return null
}
