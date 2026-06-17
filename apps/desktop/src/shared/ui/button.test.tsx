import '@testing-library/jest-dom/vitest'

import { render, screen } from '@testing-library/react'
import { describe, expect, it } from 'vitest'

import { Badge } from './badge'
import { Button } from './button'
import { Tooltip, TooltipContent, TooltipProvider, TooltipTrigger } from './tooltip'

describe('ui primitives', () => {
  it('renders Button and Badge with semantic variants', () => {
    render(
      <div>
        <Button variant="outline">Refresh</Button>
        <Badge variant="secondary">available</Badge>
      </div>,
    )

    expect(screen.getByRole('button', { name: 'Refresh' })).toBeInTheDocument()
    expect(screen.getByText('available')).toBeInTheDocument()
  })

  it('renders Tooltip with an accessible trigger', () => {
    render(
      <TooltipProvider>
        <Tooltip>
          <TooltipTrigger asChild>
            <Button variant="ghost">Info</Button>
          </TooltipTrigger>
          <TooltipContent>Harness SDK status</TooltipContent>
        </Tooltip>
      </TooltipProvider>,
    )

    expect(screen.getByRole('button', { name: 'Info' })).toBeInTheDocument()
  })
})
