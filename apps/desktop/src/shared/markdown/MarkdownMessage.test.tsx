import '@testing-library/jest-dom/vitest'

import { render, screen } from '@testing-library/react'
import { describe, expect, it } from 'vitest'

import { MarkdownMessage } from './MarkdownMessage'

describe('MarkdownMessage', () => {
  it('shows model text that looks like HTML as escaped text', () => {
    render(<MarkdownMessage>{'<think>正在整理回答</think>'}</MarkdownMessage>)

    expect(screen.getByText('<think>正在整理回答</think>')).toBeInTheDocument()
    expect(document.querySelector('think')).not.toBeInTheDocument()
  })
})
