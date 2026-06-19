import {
  FileSearch,
  FileText,
  FlaskConical,
  MessageSquarePlus,
  Settings,
  TerminalSquare,
} from 'lucide-react'
import { useEffect, useRef, useState } from 'react'

import {
  Command,
  CommandEmpty,
  CommandGroup,
  CommandInput,
  CommandItem,
  CommandList,
} from '@/shared/ui/command-menu'
import { Dialog, DialogContent, DialogTitle } from '@/shared/ui/dialog'

export type CommandPaletteAction =
  | 'new-conversation'
  | 'open-evals'
  | 'open-artifact'
  | 'search-files'
  | 'view-activity'
  | 'settings'

type CommandPaletteCommand = {
  action: CommandPaletteAction
  icon: typeof MessageSquarePlus
  label: string
}

const commands: CommandPaletteCommand[] = [
  { action: 'new-conversation', icon: MessageSquarePlus, label: 'New conversation' },
  { action: 'open-artifact', icon: FileText, label: 'Open artifact' },
  { action: 'open-evals', icon: FlaskConical, label: 'Open evals' },
  { action: 'search-files', icon: FileSearch, label: 'Search files' },
  { action: 'view-activity', icon: TerminalSquare, label: 'View activity' },
  { action: 'settings', icon: Settings, label: 'Settings' },
]

type CommandPaletteProps = {
  onAction?: (action: CommandPaletteAction) => void
}

export function CommandPalette({ onAction }: CommandPaletteProps) {
  const [open, setOpen] = useState(false)
  const previousFocusRef = useRef<HTMLElement | null>(null)
  const restoreFocusOnCloseRef = useRef(true)

  useEffect(() => {
    function onKeyDown(event: KeyboardEvent) {
      if (event.key.toLowerCase() !== 'k' || (!event.metaKey && !event.ctrlKey)) {
        return
      }

      event.preventDefault()
      setOpen((current) => {
        const nextOpen = !current
        if (nextOpen && document.activeElement instanceof HTMLElement) {
          previousFocusRef.current = document.activeElement
        }
        restoreFocusOnCloseRef.current = true
        return nextOpen
      })
    }

    window.addEventListener('keydown', onKeyDown)
    return () => window.removeEventListener('keydown', onKeyDown)
  }, [])

  function handleOpenChange(nextOpen: boolean) {
    if (nextOpen && document.activeElement instanceof HTMLElement) {
      previousFocusRef.current = document.activeElement
      restoreFocusOnCloseRef.current = true
    }
    setOpen(nextOpen)
  }

  function runCommand(action: CommandPaletteAction) {
    restoreFocusOnCloseRef.current = false
    onAction?.(action)
    setOpen(false)
  }

  return (
    <Dialog open={open} onOpenChange={handleOpenChange}>
      <DialogContent
        aria-label="Command palette"
        className="gap-0 overflow-hidden p-0"
        onCloseAutoFocus={(event) => {
          if (!restoreFocusOnCloseRef.current) {
            event.preventDefault()
            restoreFocusOnCloseRef.current = true
            return
          }

          if (previousFocusRef.current?.isConnected) {
            event.preventDefault()
            previousFocusRef.current.focus()
          }
        }}
        onOpenAutoFocus={(event) => event.preventDefault()}
      >
        <DialogTitle className="sr-only">Command palette</DialogTitle>
        <Command label="Search commands">
          <CommandInput aria-label="Search commands" autoFocus placeholder="Search commands" />
          <CommandList>
            <CommandEmpty>No commands found.</CommandEmpty>
            <CommandGroup heading="Actions">
              {commands.map(({ action, icon: Icon, label }) => (
                <CommandItem key={action} onSelect={() => runCommand(action)}>
                  <Icon aria-hidden="true" className="mr-2 size-4 text-muted-foreground" />
                  {label}
                </CommandItem>
              ))}
            </CommandGroup>
          </CommandList>
        </Command>
      </DialogContent>
    </Dialog>
  )
}
