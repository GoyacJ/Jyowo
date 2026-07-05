import { Check } from 'lucide-react'

import { cn } from '@/shared/lib/utils'

export type SlashCommand = {
  id: string
  label: string
  prompt: string
}

export const slashCommands: SlashCommand[] = [
  { id: 'plan', label: 'Plan', prompt: '/plan ' },
  { id: 'review', label: 'Review', prompt: '/review ' },
  { id: 'fix', label: 'Fix', prompt: '/fix ' },
  { id: 'explain', label: 'Explain', prompt: '/explain ' },
]

export function SlashCommandMenu({
  activeIndex,
  getCommandLabel,
  label,
  onSelect,
  open,
}: {
  activeIndex: number
  getCommandLabel: (command: SlashCommand) => string
  label: string
  onSelect: (command: SlashCommand) => void
  open: boolean
}) {
  if (!open) {
    return null
  }

  return (
    <div
      aria-label={label}
      className="mt-2 w-72 rounded-md border border-border bg-popover p-1 text-popover-foreground shadow-md"
      role="menu"
    >
      {slashCommands.map((command, index) => (
        <button
          className={cn(
            'flex w-full items-center justify-between rounded-sm px-2 py-1.5 text-left text-sm hover:bg-muted',
            index === activeIndex && 'bg-muted',
          )}
          key={command.id}
          onClick={() => onSelect(command)}
          role="menuitem"
          type="button"
        >
          <span>{getCommandLabel(command)}</span>
          {index === activeIndex ? <Check className="size-4 text-primary" /> : null}
        </button>
      ))}
    </div>
  )
}
