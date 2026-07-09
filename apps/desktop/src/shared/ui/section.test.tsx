import '@testing-library/jest-dom/vitest'

import { render, screen } from '@testing-library/react'
import { describe, expect, it } from 'vitest'

import { Section, SectionDescription, SectionHeader, SectionTitle } from './section'

describe('Section', () => {
  it('renders a token-backed section with labelled heading and description', () => {
    render(
      <Section aria-labelledby="settings-title">
        <SectionHeader>
          <SectionTitle id="settings-title">Runtime</SectionTitle>
          <SectionDescription>Configure local execution behavior.</SectionDescription>
        </SectionHeader>
      </Section>,
    )

    expect(screen.getByRole('region', { name: 'Runtime' })).toHaveClass(
      'rounded-md',
      'border-border',
      'bg-surface',
    )
    expect(screen.getByRole('heading', { name: 'Runtime' })).toHaveClass('text-section-title')
    expect(screen.getByText('Configure local execution behavior.')).toHaveClass('text-body-muted')
  })
})
