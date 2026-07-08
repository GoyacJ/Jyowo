import type { ComponentType, SVGProps } from 'react'

import { cn } from '@/shared/lib/utils'
import { Button, type ButtonProps } from '@/shared/ui/button'

export interface IconButtonProps extends Omit<ButtonProps, 'aria-label' | 'children' | 'size'> {
  icon: ComponentType<SVGProps<SVGSVGElement>>
  iconClassName?: string
  label: string
}

export function IconButton({ icon: Icon, iconClassName, label, ...props }: IconButtonProps) {
  return (
    <Button aria-label={label} size="icon" {...props}>
      <Icon aria-hidden="true" className={cn(iconClassName)} data-icon />
    </Button>
  )
}
