import '@testing-library/jest-dom/vitest'

import { act, render, screen } from '@testing-library/react'
import { describe, expect, it, vi } from 'vitest'

import { Toast } from './toast'

describe('Toast', () => {
  it('auto-closes after the configured delay', () => {
    vi.useFakeTimers()
    const onClose = vi.fn()

    try {
      render(
        <Toast autoCloseMs={4000} onClose={onClose} title="Test passed">
          Body
        </Toast>,
      )

      expect(screen.getByRole('status')).toBeInTheDocument()

      act(() => {
        vi.advanceTimersByTime(3999)
      })
      expect(onClose).not.toHaveBeenCalled()

      act(() => {
        vi.advanceTimersByTime(1)
      })
      expect(onClose).toHaveBeenCalledTimes(1)
    } finally {
      vi.useRealTimers()
    }
  })
})
