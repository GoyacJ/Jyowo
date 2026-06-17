import { describe, expect, it, vi } from 'vitest'

import {
  createPretextTextMeasurer,
  measureTextBlock,
  measureTextBlockFallback,
} from './text-measure'

describe('text-measure', () => {
  it('uses a deterministic fallback when measurement is unavailable', () => {
    expect(
      measureTextBlockFallback({
        text: 'one\ntwo',
        maxWidth: 320,
        lineHeight: 20,
        averageCharWidth: 8,
      }),
    ).toEqual({
      height: 40,
      lineCount: 2,
      measured: false,
    })
  })

  it('exposes a default measurer with fallback behavior in tests', () => {
    expect(
      measureTextBlock({
        text: 'Jyowo',
        font: '16px Inter',
        maxWidth: 1,
        lineHeight: 20,
      }).lineCount,
    ).toBeGreaterThanOrEqual(1)
  })

  it('uses the pretext measured path when an engine is provided', () => {
    const prepare = vi.fn().mockReturnValue('prepared-text')
    const layout = vi.fn().mockReturnValue({
      height: 60,
      lineCount: 3,
    })
    const measureTextBlock = createPretextTextMeasurer({ prepare, layout })

    expect(
      measureTextBlock({
        text: 'hello from Jyowo',
        font: '16px Inter',
        maxWidth: 160,
        lineHeight: 20,
      }),
    ).toEqual({
      height: 60,
      lineCount: 3,
      measured: true,
    })
    expect(prepare).toHaveBeenCalledWith('hello from Jyowo', '16px Inter', {
      whiteSpace: 'pre-wrap',
    })
    expect(layout).toHaveBeenCalledWith('prepared-text', 160, 20)
  })
})
