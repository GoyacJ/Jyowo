import { layout as pretextLayout, prepare as pretextPrepare } from '@chenglou/pretext'

export interface TextMeasureInput {
  text: string
  maxWidth: number
  lineHeight: number
  font?: string
  averageCharWidth?: number
}

export interface TextMeasureResult {
  height: number
  lineCount: number
  measured: boolean
}

type PretextPrepareOptions = Parameters<typeof pretextPrepare>[2]

export interface TextMeasureEngine<TPrepared = unknown> {
  prepare: (text: string, font: string, options?: PretextPrepareOptions) => TPrepared
  layout: (
    prepared: TPrepared,
    maxWidth: number,
    lineHeight: number,
  ) => {
    height: number
    lineCount: number
  }
}

const defaultFont = '14px Inter'
const defaultAverageCharWidth = 7

export function measureTextBlockFallback({
  text,
  maxWidth,
  lineHeight,
  averageCharWidth = defaultAverageCharWidth,
}: TextMeasureInput): TextMeasureResult {
  if (text.length === 0) {
    return {
      height: 0,
      lineCount: 0,
      measured: false,
    }
  }

  const safeWidth = Math.max(1, maxWidth)
  const lineCount = text.split('\n').reduce((count, line) => {
    const estimatedWidth = Math.max(1, line.length) * averageCharWidth
    return count + Math.max(1, Math.ceil(estimatedWidth / safeWidth))
  }, 0)

  return {
    height: lineCount * lineHeight,
    lineCount,
    measured: false,
  }
}

export function createPretextTextMeasurer<TPrepared>(engine: TextMeasureEngine<TPrepared>) {
  return function measureTextBlock(input: TextMeasureInput): TextMeasureResult {
    try {
      const prepared = engine.prepare(input.text, input.font ?? defaultFont, {
        whiteSpace: 'pre-wrap',
      })
      const measured = engine.layout(prepared, input.maxWidth, input.lineHeight)

      return {
        height: measured.height,
        lineCount: measured.lineCount,
        measured: true,
      }
    } catch {
      return measureTextBlockFallback(input)
    }
  }
}

const pretextEngine: TextMeasureEngine<ReturnType<typeof pretextPrepare>> = {
  prepare: pretextPrepare,
  layout: pretextLayout,
}

export const measureTextBlock = createPretextTextMeasurer(pretextEngine)
