import { ExternalLink, Terminal } from 'lucide-react'
import { useTranslation } from 'react-i18next'
import { useUiStore } from '@/shared/state/ui-store'
import { CommandExecutionView } from '../evidence/CommandExecutionView'
import { EvidenceDisclosure } from './evidence-disclosure'
import { useTimelineBlockDisclosure } from './timeline-disclosure-state'
import type { TimelineRenderBlock } from './timeline-render-blocks'

type CommandGroupBlock = Extract<TimelineRenderBlock, { kind: 'commandGroup' }>

export function CommandRenderBlock({
  block,
  conversationId,
  runId,
}: {
  block: CommandGroupBlock
  conversationId: string
  runId: string
}) {
  const { t } = useTranslation('conversation')
  const { open, setOpen } = useTimelineBlockDisclosure({ block, conversationId, runId })
  const setSelection = useUiStore((state) => state.setWorkbenchSelection)
  const setInspectorOpen = useUiStore((state) => state.setInspectorOpen)
  const firstCommand = block.commands[0]
  const firstCommandStep = firstCommand
    ? block.steps.find((step) => step.id === firstCommand.stepId)
    : undefined
  const firstEventRef = firstCommandStep?.eventRefs?.[0]

  return (
    <div className="grid gap-1.5">
      <EvidenceDisclosure
        actions={
          firstCommand ? (
            <button
              aria-label={t('timeline.renderBlocks.openCommand')}
              className="inline-flex size-7 items-center justify-center rounded text-muted-foreground hover:bg-muted hover:text-foreground focus-visible:ring-2 focus-visible:ring-ring"
              onClick={() => {
                setSelection({
                  kind: 'command',
                  conversationId,
                  ...(firstCommand.command.fullOutputRef
                    ? { fullOutputRef: firstCommand.command.fullOutputRef }
                    : {}),
                  ...(!firstCommand.command.fullOutputRef && firstEventRef
                    ? { eventRef: firstEventRef }
                    : {}),
                })
                setInspectorOpen(true)
              }}
              type="button"
            >
              <ExternalLink className="size-3.5" />
            </button>
          ) : null
        }
        forcedOpen={block.forcedOpen}
        icon={Terminal}
        id={block.id}
        onOpenChange={setOpen}
        open={open}
        title={t('timeline.renderBlocks.commandSummary', { count: block.commands.length })}
      >
        <div className="grid gap-2">
          {block.commands.map((command) => (
            <CommandExecutionView
              allowFullOutputFetch={false}
              command={command.command}
              conversationId={conversationId}
              density="timeline"
              key={command.id}
            />
          ))}
        </div>
      </EvidenceDisclosure>
      {!open ? (
        <ul className="grid gap-1">
          {block.commands.map((command) => (
            <li className="flex min-w-0 items-center gap-2 text-xs" key={command.id}>
              <span className="min-w-0 flex-1 truncate font-mono">{command.command.command}</span>
              {command.command.exitCode !== undefined ? (
                <span className="shrink-0 tabular-nums">
                  {t('timeline.commandEvidence.exitCode', { code: command.command.exitCode })}
                </span>
              ) : null}
            </li>
          ))}
        </ul>
      ) : null}
    </div>
  )
}
