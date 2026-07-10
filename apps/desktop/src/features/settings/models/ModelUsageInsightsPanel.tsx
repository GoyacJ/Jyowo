import { useMemo, useState } from 'react'
import { useTranslation } from 'react-i18next'

import type { ModelUsageInsightsView } from './model-settings-view-model'

type ModelUsageInsightsPanelProps = {
  insights: ModelUsageInsightsView
}

type UsageInsightsData = Extract<ModelUsageInsightsView, { status: 'ready' }>['data']
type UsageMode = 'daily' | 'weekly' | 'cumulative'

const DAY_MS = 86_400_000
const HEAT_CLASSES = [
  'bg-muted/45 dark:bg-[#232323]',
  'bg-info/25 dark:bg-[#516b82]',
  'bg-info/45 dark:bg-[#557897]',
  'bg-info/70 dark:bg-[#608eaf]',
  'bg-info dark:bg-[#71b5ff]',
] as const

export function ModelUsageInsightsPanel({ insights }: ModelUsageInsightsPanelProps) {
  const { t } = useTranslation('settings')

  return (
    <section
      aria-label={t('models.usageInsights.label')}
      className="rounded-2xl bg-surface px-4 pt-3.5 pb-2.5 sm:px-6"
    >
      {insights.status === 'ready' ? (
        <ReadyUsageInsights data={insights.data} />
      ) : (
        <div className="rounded-md border border-border bg-background p-4 text-muted-foreground text-sm">
          {insights.status === 'loading'
            ? t('models.usageInsights.loading')
            : t('models.unavailable')}
        </div>
      )}
    </section>
  )
}

function ReadyUsageInsights({ data }: { data: UsageInsightsData }) {
  const { t } = useTranslation('settings')
  const [mode, setMode] = useState<UsageMode>('daily')
  const views = ['daily', 'weekly', 'cumulative'] as const

  return (
    <div className="@container mx-auto w-full max-w-[732px]">
      <UsageMetricStrip data={data} />

      <div className="mt-9">
        <div className="mb-2 flex flex-wrap items-center justify-between gap-3">
          <h2 className="font-medium text-sm">{t('models.usageInsights.activityTitle')}</h2>
          <div
            aria-label={t('models.usageInsights.views.label')}
            className="inline-flex h-7 items-center justify-center gap-4 text-muted-foreground"
            role="tablist"
          >
            {views.map((view, index) => (
              <button
                aria-controls={`model-usage-${view}-panel`}
                aria-selected={mode === view}
                className={`inline-flex min-h-6 items-center justify-center whitespace-nowrap rounded-sm px-1 text-sm outline-none transition-colors duration-200 focus-visible:ring-2 focus-visible:ring-ring ${
                  mode === view
                    ? 'font-semibold text-foreground'
                    : 'font-normal hover:text-foreground/90'
                }`}
                id={`model-usage-${view}-tab`}
                key={view}
                onKeyDown={(event) => {
                  if (event.key !== 'ArrowLeft' && event.key !== 'ArrowRight') {
                    return
                  }
                  event.preventDefault()
                  const direction = event.key === 'ArrowRight' ? 1 : -1
                  const next = views[(index + direction + views.length) % views.length]
                  setMode(next)
                  document.getElementById(`model-usage-${next}-tab`)?.focus()
                }}
                onClick={() => setMode(view)}
                role="tab"
                tabIndex={mode === view ? 0 : -1}
                type="button"
              >
                {t(`models.usageInsights.views.${view}`)}
              </button>
            ))}
          </div>
        </div>

        <div
          aria-labelledby={`model-usage-${mode}-tab`}
          id={`model-usage-${mode}-panel`}
          role="tabpanel"
        >
          {mode === 'daily' ? <DailyTokenHeatmap data={data} /> : null}
          {mode === 'weekly' ? <WeeklyTokenChart data={data} /> : null}
          {mode === 'cumulative' ? <CumulativeTokenChart data={data} /> : null}
        </div>
      </div>
    </div>
  )
}

function UsageMetricStrip({ data }: { data: UsageInsightsData }) {
  const { i18n, t } = useTranslation('settings')
  const metrics = [
    {
      label: t('models.usageInsights.metrics.totalTokens'),
      value: formatCompactNumber(data.metrics.totalTokens, i18n.language),
    },
    {
      label: t('models.usageInsights.metrics.peakDayTokens'),
      value: formatCompactNumber(data.metrics.peakDayTokens, i18n.language),
    },
    {
      label: t('models.usageInsights.metrics.longestTaskDuration'),
      value: formatDuration(t, data.metrics.longestTaskDurationMs),
    },
    {
      label: t('models.usageInsights.metrics.currentStreak'),
      value: t('models.usageInsights.units.days', { count: data.metrics.currentStreakDays }),
    },
    {
      label: t('models.usageInsights.metrics.longestStreak'),
      value: t('models.usageInsights.units.days', { count: data.metrics.longestStreakDays }),
    },
  ]

  return (
    <dl className="grid overflow-hidden rounded-2xl border border-border/80 @min-[640px]:grid-cols-5">
      {metrics.map((metric, index) => (
        <UsageMetric index={index} key={metric.label} label={metric.label} value={metric.value} />
      ))}
    </dl>
  )
}

function UsageMetric({ index, label, value }: { index: number; label: string; value: string }) {
  return (
    <div
      className={`relative flex min-h-[60px] flex-col items-center justify-center px-3 text-center ${
        index > 0 ? 'border-border/70 border-t @min-[640px]:border-t-0' : ''
      }`}
    >
      {index > 0 ? (
        <span
          aria-hidden="true"
          className="absolute top-1/2 left-0 hidden h-9 -translate-y-1/2 border-border/70 border-l @min-[640px]:block"
        />
      ) : null}
      <dt className="order-2 mt-1 text-muted-foreground text-sm leading-none">{label}</dt>
      <dd className="order-1 max-w-full truncate font-medium text-base leading-none">{value}</dd>
    </div>
  )
}

function DailyTokenHeatmap({ data }: { data: UsageInsightsData }) {
  const { i18n, t } = useTranslation('settings')
  const slots = useMemo(() => buildHeatmapSlots(data), [data])
  const weekCount = Math.max(1, Math.ceil(slots.length / 7))

  return (
    <div className="overflow-x-auto pb-1">
      <div className="min-w-max">
        <div
          className="grid grid-flow-col grid-rows-7 gap-[3px]"
          style={{ gridTemplateColumns: `repeat(${weekCount}, 0.6875rem)` }}
        >
          {slots.map((slot) =>
            slot.entry ? (
              <button
                aria-label={t('models.usageInsights.dailyPoint', {
                  date: slot.date,
                  tokens: formatTokenCount(t, slot.entry.tokens),
                })}
                className={`size-[11px] rounded-[3px] ${HEAT_CLASSES[slot.entry.level]}`}
                data-level={slot.entry.level}
                data-testid={`usage-day-${slot.date}`}
                key={slot.date}
                title={t('models.usageInsights.dailyPoint', {
                  date: slot.date,
                  tokens: formatTokenCount(t, slot.entry.tokens),
                })}
                type="button"
              />
            ) : (
              <span aria-hidden="true" className="size-[11px]" key={slot.date} />
            ),
          )}
        </div>
        <div
          className="mt-2.5 grid gap-[3px] text-muted-foreground text-xs"
          style={{ gridTemplateColumns: `repeat(${weekCount}, 0.6875rem)` }}
        >
          {data.monthLabels.map((label) => (
            <span
              className="min-w-8 whitespace-nowrap"
              key={`${label.date}-${label.label}`}
              style={{ gridColumn: `${monthLabelColumn(slots[0]?.date, label.date)} / span 4` }}
            >
              {formatMonthLabel(label.date, i18n.language)}
            </span>
          ))}
        </div>
      </div>
    </div>
  )
}

function WeeklyTokenChart({ data }: { data: UsageInsightsData }) {
  const { t } = useTranslation('settings')
  const maxTokens = Math.max(1, ...data.weekly.map((week) => week.tokens))

  return (
    <div className="overflow-x-auto pb-1">
      <div className="flex h-48 min-w-[680px] items-end gap-2 border-border border-b px-1">
        {data.weekly.map((week) => {
          const height = `${Math.max(2, (week.tokens / maxTokens) * 100)}%`
          const label = t('models.usageInsights.weeklyPoint', {
            start: week.weekStart,
            end: week.weekEnd,
            tokens: formatTokenCount(t, week.tokens),
          })
          return (
            <div className="flex min-w-3 flex-1 items-end" key={week.weekStart}>
              <div
                aria-label={label}
                className="w-full rounded-t-[3px] bg-info/80 transition-colors hover:bg-info"
                role="img"
                style={{ height }}
                title={label}
              />
            </div>
          )
        })}
      </div>
    </div>
  )
}

function CumulativeTokenChart({ data }: { data: UsageInsightsData }) {
  const { t } = useTranslation('settings')
  const width = 720
  const height = 180
  const padding = 18
  const maxTokens = Math.max(1, ...data.cumulative.map((point) => point.tokens))
  const points = data.cumulative.map((point, index) => {
    const x =
      padding +
      (data.cumulative.length <= 1
        ? 0
        : (index / (data.cumulative.length - 1)) * (width - padding * 2))
    const y = height - padding - (point.tokens / maxTokens) * (height - padding * 2)
    return { ...point, x, y }
  })
  const path = points.map((point) => `${round(point.x)},${round(point.y)}`).join(' ')

  return (
    <div className="overflow-x-auto pb-1">
      <div className="relative min-w-[720px]">
        <svg aria-hidden="true" className="h-48 w-[720px]" viewBox={`0 0 ${width} ${height}`}>
          <line
            stroke="hsl(var(--border))"
            strokeWidth="1"
            x1={padding}
            x2={width - padding}
            y1={height - padding}
            y2={height - padding}
          />
          <polyline
            fill="none"
            points={path}
            stroke="hsl(var(--info))"
            strokeLinecap="round"
            strokeLinejoin="round"
            strokeWidth="2.5"
          />
          {points.map((point) => (
            <circle
              cx={point.x}
              cy={point.y}
              fill="hsl(var(--surface))"
              key={point.date}
              r="3"
              stroke="hsl(var(--info))"
              strokeWidth="2"
            />
          ))}
        </svg>
        <div className="absolute inset-0 h-48 w-[720px]">
          {points.map((point) => (
            <button
              aria-label={t('models.usageInsights.cumulativePoint', {
                date: point.date,
                tokens: formatTokenCount(t, point.tokens),
              })}
              className="absolute size-4 -translate-x-1/2 -translate-y-1/2 rounded-full outline-none transition-[background-color,box-shadow] duration-150 hover:bg-info/20 focus-visible:bg-info/20 focus-visible:ring-2 focus-visible:ring-ring"
              key={point.date}
              style={{ left: `${point.x}px`, top: `${point.y}px` }}
              title={t('models.usageInsights.cumulativePoint', {
                date: point.date,
                tokens: formatTokenCount(t, point.tokens),
              })}
              type="button"
            />
          ))}
        </div>
      </div>
    </div>
  )
}

function buildHeatmapSlots(data: UsageInsightsData) {
  const entryByDate = new Map(data.daily.map((entry) => [entry.date, entry]))
  const start = mondayStart(data.rangeStart)
  const end = data.rangeEnd
  const slots: { date: string; entry: UsageInsightsData['daily'][number] | null }[] = []

  for (
    let cursor = localDateUtcMs(start), endMs = localDateUtcMs(end);
    cursor <= endMs;
    cursor += DAY_MS
  ) {
    const date = formatLocalDateUtcMs(cursor)
    slots.push({ date, entry: entryByDate.get(date) ?? null })
  }

  return slots
}

function mondayStart(date: string): string {
  const utc = localDateUtcMs(date)
  const day = new Date(utc).getUTCDay()
  const daysSinceMonday = (day + 6) % 7
  return formatLocalDateUtcMs(utc - daysSinceMonday * DAY_MS)
}

function monthLabelColumn(startDate: string | undefined, labelDate: string): number {
  if (!startDate) {
    return 1
  }
  return Math.floor((localDateUtcMs(labelDate) - localDateUtcMs(startDate)) / DAY_MS / 7) + 1
}

function localDateUtcMs(date: string): number {
  return Date.UTC(Number(date.slice(0, 4)), Number(date.slice(5, 7)) - 1, Number(date.slice(8, 10)))
}

function formatLocalDateUtcMs(utcMs: number): string {
  return new Date(utcMs).toISOString().slice(0, 10)
}

function formatMonthLabel(date: string, locale: string): string {
  return new Intl.DateTimeFormat(locale, { month: 'short', timeZone: 'UTC' }).format(
    new Date(`${date}T00:00:00Z`),
  )
}

function formatTokenCount(
  t: (key: string, options: { tokens: string }) => string,
  tokens: number,
): string {
  return t('models.usageInsights.tokenCount', { tokens: formatNumber(tokens) })
}

function formatNumber(value: number): string {
  return new Intl.NumberFormat().format(value)
}

function formatCompactNumber(value: number, locale: string): string {
  return new Intl.NumberFormat(locale, {
    maximumFractionDigits: 1,
    notation: 'compact',
  }).format(value)
}

function formatDuration(
  t: (key: string, options: { count: number }) => string,
  durationMs: number,
): string {
  if (durationMs <= 0) {
    return t('models.usageInsights.units.seconds', { count: 0 })
  }
  const totalSeconds = Math.round(durationMs / 1000)
  const hours = Math.floor(totalSeconds / 3600)
  const minutes = Math.floor((totalSeconds % 3600) / 60)
  const seconds = totalSeconds % 60
  return [
    hours > 0 ? t('models.usageInsights.units.hours', { count: hours }) : null,
    minutes > 0 ? t('models.usageInsights.units.minutes', { count: minutes }) : null,
    seconds > 0 || (hours === 0 && minutes === 0)
      ? t('models.usageInsights.units.seconds', { count: seconds })
      : null,
  ]
    .filter(Boolean)
    .join(' ')
}

function round(value: number): number {
  return Math.round(value * 10) / 10
}
