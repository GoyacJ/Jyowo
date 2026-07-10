import '@testing-library/jest-dom/vitest'

import { fireEvent, render, screen } from '@testing-library/react'
import { describe, expect, it } from 'vitest'

import { AppI18nProvider } from '@/shared/i18n/i18n'
import { uiStore } from '@/shared/state/ui-store'
import { ModelUsageInsightsPanel } from './ModelUsageInsightsPanel'
import type { ModelUsageInsightsView } from './model-settings-view-model'

const readyInsights: ModelUsageInsightsView = {
  status: 'ready',
  data: {
    rangeStart: '2026-06-24',
    rangeEnd: '2026-06-30',
    metrics: {
      totalTokens: 2100,
      peakDayTokens: 40,
      longestTaskDurationMs: 61_000,
      currentStreakDays: 2,
      longestStreakDays: 3,
    },
    daily: [
      { date: '2026-06-24', usage: usage(5, 0), tokens: 5, level: 1 },
      { date: '2026-06-25', usage: usage(0, 0), tokens: 0, level: 0 },
      { date: '2026-06-26', usage: usage(7, 3), tokens: 10, level: 1 },
      { date: '2026-06-27', usage: usage(10, 10), tokens: 20, level: 2 },
      { date: '2026-06-28', usage: usage(0, 0), tokens: 0, level: 0 },
      { date: '2026-06-29', usage: usage(20, 10), tokens: 30, level: 3 },
      { date: '2026-06-30', usage: usage(30, 10), tokens: 40, level: 4 },
    ],
    monthLabels: [{ date: '2026-06-24', label: 'Jun' }],
    weekly: [
      { weekStart: '2026-06-22', weekEnd: '2026-06-28', tokens: 35 },
      { weekStart: '2026-06-29', weekEnd: '2026-07-05', tokens: 70 },
    ],
    cumulative: [
      { date: '2026-06-24', tokens: 5 },
      { date: '2026-06-25', tokens: 5 },
      { date: '2026-06-26', tokens: 15 },
      { date: '2026-06-27', tokens: 35 },
      { date: '2026-06-28', tokens: 35 },
      { date: '2026-06-29', tokens: 65 },
      { date: '2026-06-30', tokens: 105 },
    ],
  },
}

describe('ModelUsageInsightsPanel', () => {
  it('renders token metrics and daily heatmap cells', () => {
    renderPanel(readyInsights)

    const panel = screen.getByLabelText('Token activity')
    expect(panel).toHaveTextContent('2,100 tokens')
    expect(panel).toHaveTextContent('40 tokens')
    expect(panel).toHaveTextContent('1m 1s')
    expect(panel).toHaveTextContent('2 days')
    expect(panel).toHaveTextContent('3 days')

    const peakCell = screen.getByTestId('usage-day-2026-06-30')
    expect(peakCell).toHaveAttribute('data-level', '4')
    expect(peakCell).toHaveAttribute('title', '2026-06-30 · 40 tokens')
    expect(screen.getByText('Jun')).toBeInTheDocument()
  })

  it('switches between weekly and cumulative views', () => {
    renderPanel(readyInsights)

    fireEvent.click(screen.getByRole('tab', { name: 'Weekly' }))
    expect(
      screen.getByRole('img', { name: '2026-06-22 to 2026-06-28 · 35 tokens' }),
    ).toBeInTheDocument()
    expect(
      screen.getByRole('img', { name: '2026-06-29 to 2026-07-05 · 70 tokens' }),
    ).toBeInTheDocument()

    fireEvent.click(screen.getByRole('tab', { name: 'Cumulative' }))
    expect(
      screen.getByRole('button', { name: '2026-06-30 · 105 tokens cumulative' }),
    ).toBeInTheDocument()
  })

  it('localizes token chart labels', () => {
    renderPanel(readyInsights, 'zh-CN')

    expect(screen.getByLabelText('Token 活动')).toHaveTextContent('2,100 个 Token')
    expect(screen.getByLabelText('Token 活动')).toHaveTextContent('1 分 1 秒')
    expect(screen.getByTestId('usage-day-2026-06-30')).toHaveAttribute(
      'title',
      '2026-06-30 · 40 个 Token',
    )

    fireEvent.click(screen.getByRole('tab', { name: '每周' }))
    expect(
      screen.getByRole('img', { name: '2026-06-22 至 2026-06-28 · 35 个 Token' }),
    ).toBeInTheDocument()

    fireEvent.click(screen.getByRole('tab', { name: '累计' }))
    expect(
      screen.getByRole('button', { name: '2026-06-30 · 累计 105 个 Token' }),
    ).toBeInTheDocument()
  })

  it('renders non-ready states without charts', () => {
    renderPanel({ status: 'unavailable' })

    expect(screen.getByLabelText('Token activity')).toHaveTextContent('Unavailable')
    expect(screen.queryByTestId('usage-day-2026-06-30')).not.toBeInTheDocument()
  })
})

function renderPanel(insights: ModelUsageInsightsView, locale: 'en-US' | 'zh-CN' = 'en-US') {
  uiStore.getState().setLocale(locale)
  return render(
    <AppI18nProvider>
      <ModelUsageInsightsPanel insights={insights} />
    </AppI18nProvider>,
  )
}

function usage(inputTokens: number, outputTokens: number) {
  return {
    cacheReadTokens: 0,
    cacheWriteTokens: 0,
    costMicros: 0,
    inputTokens,
    outputTokens,
    toolCalls: 0,
  }
}
