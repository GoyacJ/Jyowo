import '@testing-library/jest-dom/vitest'

import { render, screen } from '@testing-library/react'
import type { SVGProps } from 'react'
import { describe, expect, it } from 'vitest'

import { IconButton } from './icon-button'

function TestIcon(props: SVGProps<SVGSVGElement>) {
  return <svg {...props} />
}

describe('IconButton', () => {
  it('renders an accessible icon-only button with fixed icon sizing', () => {
    render(<IconButton icon={TestIcon} label="Refresh workspace" variant="outline" />)

    const button = screen.getByRole('button', { name: 'Refresh workspace' })
    expect(button).toHaveClass('size-9')
    expect(button.querySelector('svg')).toHaveAttribute('aria-hidden', 'true')
    expect(button.querySelector('svg')).toHaveAttribute('data-icon', 'true')
  })

  it('allows icon-specific classes without changing button classes', () => {
    render(
      <IconButton
        icon={TestIcon}
        iconClassName="text-destructive"
        label="Delete workspace"
        variant="ghost"
      />,
    )

    const button = screen.getByRole('button', { name: 'Delete workspace' })
    expect(button).toHaveClass('size-9')
    expect(button.querySelector('svg')).toHaveClass('text-destructive')
  })
})
