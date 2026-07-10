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
      totalTokens: 12_730_000_000,
      peakDayTokens: 930_000_000,
      longestTaskDurationMs: 59_280_000,
      currentStreakDays: 3,
      longestStreakDays: 18,
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
  it('renders the compact metric strip and daily heatmap hierarchy', () => {
    renderPanel(readyInsights)

    const panel = screen.getByLabelText('Token activity')
    expect(panel.querySelectorAll('dt')).toHaveLength(5)
    expect(panel.querySelectorAll('dd')).toHaveLength(5)
    expect(panel.querySelectorAll('[data-icon]')).toHaveLength(0)
    expect(panel).toHaveTextContent('12.7B')
    expect(panel).toHaveTextContent('930M')
    expect(panel).not.toHaveTextContent('12.7B tokens')
    expect(panel).toHaveTextContent('16h 28m')
    expect(panel).toHaveTextContent('3 days')
    expect(panel).toHaveTextContent('18 days')
    expect(screen.queryByText('2026-06-24 to 2026-06-30')).not.toBeInTheDocument()
    expect(screen.queryByText('Mon')).not.toBeInTheDocument()

    const peakCell = screen.getByTestId('usage-day-2026-06-30')
    expect(peakCell).toHaveAttribute('data-level', '4')
    expect(peakCell).toHaveAttribute('title', '2026-06-30 · 40 tokens')
    expect(screen.getByText('Jun')).toBeInTheDocument()
  })

  it('switches between weekly and cumulative views', () => {
    renderPanel(readyInsights)

    const dailyTab = screen.getByRole('tab', { name: 'Daily' })
    const weeklyTab = screen.getByRole('tab', { name: 'Weekly' })
    expect(dailyTab).toHaveAttribute('tabindex', '0')
    expect(weeklyTab).toHaveAttribute('tabindex', '-1')

    fireEvent.click(weeklyTab)
    expect(dailyTab).toHaveAttribute('tabindex', '-1')
    expect(weeklyTab).toHaveAttribute('tabindex', '0')
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

    expect(screen.getByLabelText('Token 活动')).toHaveTextContent('127.3亿')
    expect(screen.getByLabelText('Token 活动')).toHaveTextContent('9.3亿')
    expect(screen.getByLabelText('Token 活动')).toHaveTextContent('16 小时 28 分')
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
