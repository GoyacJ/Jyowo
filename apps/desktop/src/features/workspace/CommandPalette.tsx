import { CalendarClock, MessageSquarePlus, Settings } from 'lucide-react'
import { useEffect, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'

import {
  Command,
  CommandEmpty,
  CommandGroup,
  CommandInput,
  CommandItem,
  CommandList,
} from '@/shared/ui/command-menu'
import { Dialog, DialogContent, DialogTitle } from '@/shared/ui/dialog'

export type CommandPaletteAction = 'new-conversation' | 'scheduled-tasks' | 'settings'

type CommandPaletteCommand = {
  action: CommandPaletteAction
  icon: typeof MessageSquarePlus
  labelKey: string
}

const commands: CommandPaletteCommand[] = [
  {
    action: 'new-conversation',
    icon: MessageSquarePlus,
    labelKey: 'commandPalette.newConversation',
  },
  {
    action: 'scheduled-tasks',
    icon: CalendarClock,
    labelKey: 'commandPalette.scheduledTasks',
  },
  { action: 'settings', icon: Settings, labelKey: 'commandPalette.settings' },
]

export const OPEN_COMMAND_PALETTE_EVENT = 'jyowo:open-command-palette'

type CommandPaletteProps = {
  onAction?: (action: CommandPaletteAction) => void
}

export function CommandPalette({ onAction }: CommandPaletteProps) {
  const { t } = useTranslation('shell')
  const [open, setOpen] = useState(false)
  const previousFocusRef = useRef<HTMLElement | null>(null)
  const restoreFocusOnCloseRef = useRef(true)

  useEffect(() => {
    function captureOpeningFocus() {
      if (document.activeElement instanceof HTMLElement) {
        previousFocusRef.current = document.activeElement
      }
      restoreFocusOnCloseRef.current = true
    }

    function openPalette() {
      captureOpeningFocus()
      setOpen(true)
    }

    function onKeyDown(event: KeyboardEvent) {
      if (event.key.toLowerCase() !== 'k' || (!event.metaKey && !event.ctrlKey)) {
        return
      }

      event.preventDefault()
      setOpen((current) => {
        const nextOpen = !current
        if (nextOpen) {
          captureOpeningFocus()
        }
        return nextOpen
      })
    }

    function onOpenCommandPalette() {
      openPalette()
    }

    window.addEventListener('keydown', onKeyDown)
    window.addEventListener(OPEN_COMMAND_PALETTE_EVENT, onOpenCommandPalette)
    return () => {
      window.removeEventListener('keydown', onKeyDown)
      window.removeEventListener(OPEN_COMMAND_PALETTE_EVENT, onOpenCommandPalette)
    }
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
        aria-label={t('commandPalette.title')}
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
        <DialogTitle className="sr-only">{t('commandPalette.title')}</DialogTitle>
        <Command label={t('commandPalette.searchLabel')}>
          <CommandInput
            aria-label={t('commandPalette.searchLabel')}
            autoFocus
            className="pr-8"
            placeholder={t('commandPalette.searchPlaceholder')}
          />
          <CommandList>
            <CommandEmpty>{t('commandPalette.empty')}</CommandEmpty>
            <CommandGroup heading={t('commandPalette.actionsHeading')}>
              {commands.map(({ action, icon: Icon, labelKey }) => (
                <CommandItem key={action} onSelect={() => runCommand(action)}>
                  <Icon aria-hidden="true" className="mr-2 size-4 text-muted-foreground" />
                  {t(labelKey)}
                </CommandItem>
              ))}
            </CommandGroup>
          </CommandList>
        </Command>
      </DialogContent>
    </Dialog>
  )
}
