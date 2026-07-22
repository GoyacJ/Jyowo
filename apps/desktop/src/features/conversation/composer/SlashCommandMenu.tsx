import type { LucideIcon } from 'lucide-react'
import { BookOpenText, ListTodo, ScanSearch, SearchX, Wrench } from 'lucide-react'

import { cn } from '@/shared/lib/utils'

import { ComposerSuggestionPanel } from './ComposerSuggestionPanel'

export type SlashCommand = {
  id: string
  icon: LucideIcon
  prompt: string
}

export const slashCommands: SlashCommand[] = [
  { id: 'plan', icon: ListTodo, prompt: '/plan ' },
  { id: 'review', icon: ScanSearch, prompt: '/review ' },
  { id: 'fix', icon: Wrench, prompt: '/fix ' },
  { id: 'explain', icon: BookOpenText, prompt: '/explain ' },
]

export const slashCommandListboxId = 'composer-slash-command-listbox'

export function slashCommandOptionId(command: SlashCommand) {
  return `composer-slash-command-${command.id}`
}

export function SlashCommandMenu({
  activeIndex,
  commands,
  emptyLabel,
  getCommandDescription,
  getCommandLabel,
  keyboardHint,
  label,
  onSelect,
  open,
}: {
  activeIndex: number
  commands: SlashCommand[]
  emptyLabel: string
  getCommandDescription: (command: SlashCommand) => string
  getCommandLabel: (command: SlashCommand) => string
  keyboardHint: string
  label: string
  onSelect: (command: SlashCommand) => void
  open: boolean
}) {
  if (!open) {
    return null
  }

  return (
    <ComposerSuggestionPanel keyboardHint={keyboardHint}>
      <div
        aria-label={label}
        className="max-h-[min(360px,45vh)] overflow-y-auto p-1.5"
        id={slashCommandListboxId}
        role="listbox"
      >
        {commands.length === 0 ? (
          <div className="flex items-center gap-2 px-3 py-5 text-muted-foreground text-sm">
            <SearchX aria-hidden="true" className="size-4 shrink-0" />
            <span>{emptyLabel}</span>
          </div>
        ) : null}
        {commands.map((command, index) => {
          const Icon = command.icon
          const selected = index === activeIndex

          return (
            <button
              aria-label={getCommandLabel(command)}
              aria-selected={selected}
              className={cn(
                'flex min-h-11 w-full items-center gap-3 rounded-md px-3 py-2 text-left outline-none transition-colors hover:bg-muted',
                selected && 'bg-muted text-foreground',
              )}
              id={slashCommandOptionId(command)}
              key={command.id}
              onClick={() => onSelect(command)}
              role="option"
              type="button"
            >
              <Icon aria-hidden="true" className="size-4 shrink-0 text-muted-foreground" />
              <span className="flex min-w-0 flex-1 items-center gap-3">
                <span className="flex w-36 shrink-0 items-baseline gap-2">
                  <span className="font-medium font-mono text-foreground text-sm">
                    /{command.id}
                  </span>
                  <span className="truncate text-muted-foreground text-sm">
                    {getCommandLabel(command)}
                  </span>
                </span>
                <span className="min-w-0 flex-1 truncate text-muted-foreground text-sm">
                  {getCommandDescription(command)}
                </span>
              </span>
            </button>
          )
        })}
      </div>
    </ComposerSuggestionPanel>
  )
}
