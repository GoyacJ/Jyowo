import * as PopoverPrimitive from '@radix-ui/react-popover'
import type { ComponentProps } from 'react'

import { cn } from '@/shared/lib/utils'

export const Popover = PopoverPrimitive.Root
export const PopoverTrigger = PopoverPrimitive.Trigger

export function PopoverContent({
  align = 'center',
  className,
  sideOffset = 4,
  ...props
}: ComponentProps<typeof PopoverPrimitive.Content>) {
  return (
    <PopoverPrimitive.Portal>
      <PopoverPrimitive.Content
        align={align}
        className={cn(
          'z-50 w-72 rounded-md border border-border bg-popover p-4 text-popover-foreground shadow-md outline-none',
          className,
        )}
        sideOffset={sideOffset}
        {...props}
      />
    </PopoverPrimitive.Portal>
  )
}
