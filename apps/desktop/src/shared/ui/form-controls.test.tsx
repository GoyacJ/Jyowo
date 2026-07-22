import '@testing-library/jest-dom/vitest'

import { render, screen } from '@testing-library/react'
import { describe, expect, it } from 'vitest'

import { EmptyState } from './empty-state'
import { FieldControl, FieldDescription, FieldError } from './field'
import { CheckboxCard, RadioCard } from './radio-card-group'
import { Select } from './select'
import { Textarea } from './textarea'

describe('form controls', () => {
  it('renders token-backed field controls', () => {
    render(
      <FieldControl fieldId="model-filter" label="Provider">
        <Select id="model-filter">
          <option>All providers</option>
        </Select>
        <FieldDescription>Choose a provider.</FieldDescription>
        <FieldError>Provider is unavailable.</FieldError>
      </FieldControl>,
    )

    expect(screen.getByLabelText('Provider')).toHaveClass('border-input', 'bg-background')
    expect(screen.getByText('Choose a provider.')).toHaveClass('text-body-muted')
    expect(screen.getByText('Provider is unavailable.')).toHaveClass('text-destructive')
  })

  it('renders textarea, radio card, and empty state primitives', () => {
    render(
      <div>
        <Textarea aria-label="Prompt" />
        <RadioCard name="mode" value="auto">
          <span>Auto</span>
        </RadioCard>
        <CheckboxCard name="targets" value="desktop">
          <span>Desktop</span>
        </CheckboxCard>
        <EmptyState>No items</EmptyState>
      </div>,
    )

    expect(screen.getByLabelText('Prompt')).toHaveClass('border-input', 'bg-background')
    expect(screen.getByLabelText('Auto')).toHaveAttribute('type', 'radio')
    expect(screen.getByLabelText('Desktop')).toHaveAttribute('type', 'checkbox')
    expect(screen.getByText('No items')).toHaveClass('border-dashed', 'bg-background')
  })
})
