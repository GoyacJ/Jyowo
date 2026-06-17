import type { Meta, StoryObj } from '@storybook/react-vite'

import { SystemStatusPage } from '@/features/system-status/SystemStatusPage'
import { Badge } from '@/shared/ui/badge'
import { Button } from '@/shared/ui/button'
import { Tooltip, TooltipContent, TooltipProvider, TooltipTrigger } from '@/shared/ui/tooltip'

const meta = {
  title: 'Foundation/Smoke',
  component: SystemStatusPage,
} satisfies Meta<typeof SystemStatusPage>

export default meta

type Story = StoryObj<typeof meta>

export const SystemStatus: Story = {}

export const Primitives: Story = {
  render: () => (
    <div className="flex items-center gap-3">
      <Button variant="outline">Refresh</Button>
      <Badge variant="secondary">available</Badge>
      <TooltipProvider>
        <Tooltip>
          <TooltipTrigger asChild>
            <Button variant="ghost">Info</Button>
          </TooltipTrigger>
          <TooltipContent>Harness SDK status</TooltipContent>
        </Tooltip>
      </TooltipProvider>
    </div>
  ),
}
