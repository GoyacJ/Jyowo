import type { Meta, StoryObj } from '@storybook/react-vite'
import { Info, Trash2 } from 'lucide-react'
import { useEffect, useState } from 'react'

import { SystemStatusPage } from '@/features/system-status/SystemStatusPage'
import { highlightCode } from '@/shared/code/highlight'
import {
  loadUiPreferencesStore,
  readUiPreferences,
  UI_PREFERENCES_STORE_PATH,
  type UiPreferences,
  type UiThemePreference,
  writeUiPreferences,
} from '@/shared/local-store/ui-preferences-store'
import { MarkdownMessage } from '@/shared/markdown/MarkdownMessage'
import { Badge } from '@/shared/ui/badge'
import { Button } from '@/shared/ui/button'
import { Checkbox } from '@/shared/ui/checkbox'
import {
  Command,
  CommandEmpty,
  CommandGroup,
  CommandInput,
  CommandItem,
  CommandList,
  CommandSeparator,
} from '@/shared/ui/command-menu'
import {
  Dialog,
  DialogClose,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
  DialogTrigger,
} from '@/shared/ui/dialog'
import {
  DropdownMenu,
  DropdownMenuCheckboxItem,
  DropdownMenuContent,
  DropdownMenuGroup,
  DropdownMenuItem,
  DropdownMenuLabel,
  DropdownMenuSeparator,
  DropdownMenuSub,
  DropdownMenuSubContent,
  DropdownMenuSubTrigger,
  DropdownMenuTrigger,
} from '@/shared/ui/dropdown-menu'
import { IconButton } from '@/shared/ui/icon-button'
import { Popover, PopoverContent, PopoverTrigger } from '@/shared/ui/popover'
import { ResizableHandle, ResizablePanel, ResizablePanelGroup } from '@/shared/ui/resizable-panels'
import { ScrollArea } from '@/shared/ui/scroll-area'
import { Section, SectionDescription, SectionHeader, SectionTitle } from '@/shared/ui/section'
import { StatusBadge } from '@/shared/ui/status-badge'
import { Switch } from '@/shared/ui/switch'
import { Tabs, TabsContent, TabsList, TabsTrigger } from '@/shared/ui/tabs'
import { Tooltip, TooltipContent, TooltipProvider, TooltipTrigger } from '@/shared/ui/tooltip'

const meta = {
  title: 'Foundation/Smoke',
  component: SystemStatusPage,
} satisfies Meta<typeof SystemStatusPage>

export default meta

type Story = StoryObj<typeof meta>

const uiPreferencesSmoke: UiPreferences = {
  theme: 'system',
  locale: 'zh-CN',
  sidebarCollapsed: false,
  chatComposerHeight: 160,
  contextPanelWidth: 320,
}
const uiThemeSmoke: UiThemePreference = uiPreferencesSmoke.theme
const uiStoreApiNames = [
  loadUiPreferencesStore.name,
  readUiPreferences.name,
  writeUiPreferences.name,
]

export const SystemStatus: Story = {}

export const Primitives: Story = {
  render: () => (
    <div className="grid max-w-4xl gap-5 p-4">
      <Section>
        <SectionHeader>
          <SectionTitle>Design tokens</SectionTitle>
          <SectionDescription>
            Semantic color, typography, status, and action primitives.
          </SectionDescription>
        </SectionHeader>
        <div className="grid gap-4 md:grid-cols-[1fr_1fr]">
          <div className="grid gap-2">
            <p className="text-caption">Color tokens</p>
            <div className="grid grid-cols-2 gap-2 text-sm">
              {[
                ['Background', 'bg-background text-foreground'],
                ['Surface', 'bg-surface text-foreground'],
                ['Primary', 'bg-primary text-primary-foreground'],
                ['Success', 'bg-success text-success-foreground'],
                ['Warning', 'bg-warning text-warning-foreground'],
                ['Destructive', 'bg-destructive text-destructive-foreground'],
              ].map(([label, className]) => (
                <div
                  className={`rounded-md border border-border px-3 py-2 ${className}`}
                  key={label}
                >
                  {label}
                </div>
              ))}
            </div>
          </div>
          <div className="grid gap-3">
            <p className="text-page-title">Page title</p>
            <p className="text-section-title">Section title</p>
            <p className="text-body-muted">Muted body copy uses a shared typography token.</p>
            <div className="flex flex-wrap items-center gap-2">
              <StatusBadge tone="neutral">idle</StatusBadge>
              <StatusBadge tone="success">running</StatusBadge>
              <StatusBadge tone="warning">warning</StatusBadge>
              <StatusBadge tone="destructive">failed</StatusBadge>
              <StatusBadge tone="info">checking</StatusBadge>
              <IconButton icon={Info} label="Inspect token" variant="outline" />
              <IconButton icon={Trash2} label="Remove token" variant="ghost" />
            </div>
          </div>
        </div>
      </Section>

      <div className="flex flex-wrap items-center gap-3">
        <Button variant="outline">Refresh</Button>
        <Badge variant="secondary">available</Badge>
        <Checkbox aria-label="Keep local workspace state visible" defaultChecked />
        <Switch aria-label="Use compact mode" defaultChecked />
        <TooltipProvider>
          <Tooltip>
            <TooltipTrigger asChild>
              <Button variant="ghost">Info</Button>
            </TooltipTrigger>
            <TooltipContent>Harness SDK status</TooltipContent>
          </Tooltip>
        </TooltipProvider>
      </div>

      <div className="flex flex-wrap items-center gap-3">
        <Dialog>
          <DialogTrigger asChild>
            <Button variant="outline">Dialog</Button>
          </DialogTrigger>
          <DialogContent>
            <DialogHeader>
              <DialogTitle>Workspace preferences</DialogTitle>
              <DialogDescription>
                UI preferences are stored in {UI_PREFERENCES_STORE_PATH}.
              </DialogDescription>
            </DialogHeader>
            <DialogFooter>
              <DialogClose asChild>
                <Button variant="ghost">Cancel</Button>
              </DialogClose>
              <Button>Save</Button>
            </DialogFooter>
          </DialogContent>
        </Dialog>

        <DropdownMenu>
          <DropdownMenuTrigger asChild>
            <Button variant="outline">Menu</Button>
          </DropdownMenuTrigger>
          <DropdownMenuContent>
            <DropdownMenuLabel>Workspace</DropdownMenuLabel>
            <DropdownMenuGroup>
              <DropdownMenuItem>Open session</DropdownMenuItem>
              <DropdownMenuCheckboxItem checked>Natural chat</DropdownMenuCheckboxItem>
            </DropdownMenuGroup>
            <DropdownMenuSeparator />
            <DropdownMenuSub>
              <DropdownMenuSubTrigger>Mode</DropdownMenuSubTrigger>
              <DropdownMenuSubContent>
                <DropdownMenuItem>Chat first</DropdownMenuItem>
              </DropdownMenuSubContent>
            </DropdownMenuSub>
            <DropdownMenuItem>Settings</DropdownMenuItem>
          </DropdownMenuContent>
        </DropdownMenu>

        <Popover>
          <PopoverTrigger asChild>
            <Button variant="outline">Popover</Button>
          </PopoverTrigger>
          <PopoverContent className="text-sm leading-6">
            Natural chat is the product surface. Tasks remain internal structure.
          </PopoverContent>
        </Popover>
      </div>

      <Tabs defaultValue="chat">
        <TabsList>
          <TabsTrigger value="chat">Chat</TabsTrigger>
          <TabsTrigger value="context">Context</TabsTrigger>
        </TabsList>
        <TabsContent value="chat">
          <MarkdownMessage>
            {
              'Use **Markdown** for assistant messages.\n\n- GFM lists\n- Links stay explicit\n\n`raw HTML` is ignored.'
            }
          </MarkdownMessage>
        </TabsContent>
        <TabsContent value="context">
          <ScrollArea className="h-28 rounded-md border border-border p-3">
            <p className="text-muted-foreground text-sm leading-6">
              Context, memories, and files should stay readable without turning the product into an
              admin console.
            </p>
            <p className="mt-3 text-muted-foreground text-xs">
              Store API: {uiStoreApiNames.join(', ')}. Theme: {uiThemeSmoke}.
            </p>
          </ScrollArea>
        </TabsContent>
      </Tabs>

      <div className="grid gap-5 md:grid-cols-[1fr_1fr]">
        <Command className="h-48 border border-border">
          <CommandInput placeholder="Search actions..." />
          <CommandList>
            <CommandEmpty>No action found.</CommandEmpty>
            <CommandGroup heading="Actions">
              <CommandItem>Start chat</CommandItem>
              <CommandItem>Attach file</CommandItem>
              <CommandSeparator />
              <CommandItem>Open command</CommandItem>
            </CommandGroup>
          </CommandList>
        </Command>

        <ResizablePanelGroup
          className="h-48 rounded-md border border-border"
          orientation="horizontal"
        >
          <ResizablePanel defaultSize="58%">
            <div className="flex h-full items-center justify-center text-muted-foreground text-sm">
              Conversation
            </div>
          </ResizablePanel>
          <ResizableHandle />
          <ResizablePanel defaultSize="42%">
            <HighlightedCodeSmoke />
          </ResizablePanel>
        </ResizablePanelGroup>
      </div>
    </div>
  ),
}

function HighlightedCodeSmoke() {
  const [html, setHtml] = useState('')

  useEffect(() => {
    let disposed = false

    highlightCode('const surface = "chat"', { lang: 'ts' }).then((highlighted) => {
      if (!disposed) {
        setHtml(highlighted)
      }
    })

    return () => {
      disposed = true
    }
  }, [])

  return (
    <div
      className="h-full overflow-auto p-3 text-xs [&_pre]:m-0 [&_pre]:bg-transparent [&_pre]:p-0"
      dangerouslySetInnerHTML={{ __html: html }}
    />
  )
}
