import type { ComponentProps } from 'react'
import { Group, Panel, Separator } from 'react-resizable-panels'

import { cn } from '@/shared/lib/utils'

export function ResizablePanelGroup({ className, ...props }: ComponentProps<typeof Group>) {
  return <Group className={cn('flex size-full', className)} {...props} />
}

export const ResizablePanel = Panel

export function ResizableHandle({ className, ...props }: ComponentProps<typeof Separator>) {
  return (
    <Separator
      className={cn(
        'relative flex w-px items-center justify-center bg-border outline-none transition-colors hover:bg-ring focus-visible:ring-2 focus-visible:ring-ring data-[orientation=vertical]:h-px data-[orientation=vertical]:w-full',
        className,
      )}
      {...props}
    />
  )
}
