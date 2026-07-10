import { Activity, CalendarDays, Flame, Timer, Trophy } from 'lucide-react'
import type { ReactNode } from 'react'
import { useMemo, useState } from 'react'
import { useTranslation } from 'react-i18next'

import type { ModelUsageInsightsView } from './model-settings-view-model'

type ModelUsageInsightsPanelProps = {
  insights: ModelUsageInsightsView
}

type UsageInsightsData = Extract<ModelUsageInsightsView, { status: 'ready' }>['data']
type UsageMode = 'daily' | 'weekly' | 'cumulative'

const DAY_MS = 86_400_000
const HEAT_CLASSES = ['bg-muted/45', 'bg-info/25', 'bg-info/45', 'bg-info/70', 'bg-info'] as const

export function ModelUsageInsightsPanel({ insights }: ModelUsageInsightsPanelProps) {
  const { t } = useTranslation('settings')

  return (
    <section
      aria-label={t('models.usageInsights.label')}
      className="rounded-md border border-border bg-surface p-4"
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
    <div className="space-y-4">
      <UsageMetricStrip data={data} />

      <div className="rounded-md border border-border bg-background p-3">
        <div className="mb-3 flex flex-wrap items-center justify-between gap-3">
          <div>
            <h2 className="font-medium text-sm">{t('models.usageInsights.activityTitle')}</h2>
            <p className="text-muted-foreground text-xs">
              {t('models.usageInsights.range', {
                start: data.rangeStart,
                end: data.rangeEnd,
              })}
            </p>
          </div>
          <div
            aria-label={t('models.usageInsights.views.label')}
            className="inline-flex h-8 items-center justify-center rounded-md bg-muted p-1 text-muted-foreground"
            role="tablist"
          >
            {views.map((view, index) => (
              <button
                aria-controls={`model-usage-${view}-panel`}
                aria-selected={mode === view}
                className={`inline-flex items-center justify-center whitespace-nowrap rounded-md px-2.5 py-0.5 font-medium text-xs outline-none transition-[background-color,color,box-shadow] duration-200 focus-visible:ring-2 focus-visible:ring-ring ${
                  mode === view
                    ? 'bg-surface text-foreground shadow-sm'
                    : 'hover:text-foreground/90'
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
  const { t } = useTranslation('settings')
  const metrics = [
    {
      icon: <Activity aria-hidden="true" className="size-4" data-icon />,
      label: t('models.usageInsights.metrics.totalTokens'),
      value: formatTokenCount(t, data.metrics.totalTokens),
    },
    {
      icon: <Flame aria-hidden="true" className="size-4" data-icon />,
      label: t('models.usageInsights.metrics.peakDayTokens'),
      value: formatTokenCount(t, data.metrics.peakDayTokens),
    },
    {
      icon: <Timer aria-hidden="true" className="size-4" data-icon />,
      label: t('models.usageInsights.metrics.longestTaskDuration'),
      value: formatDuration(t, data.metrics.longestTaskDurationMs),
    },
    {
      icon: <CalendarDays aria-hidden="true" className="size-4" data-icon />,
      label: t('models.usageInsights.metrics.currentStreak'),
      value: t('models.usageInsights.units.days', { count: data.metrics.currentStreakDays }),
    },
    {
      icon: <Trophy aria-hidden="true" className="size-4" data-icon />,
      label: t('models.usageInsights.metrics.longestStreak'),
      value: t('models.usageInsights.units.days', { count: data.metrics.longestStreakDays }),
    },
  ]

  return (
    <div className="grid gap-3 sm:grid-cols-2 xl:grid-cols-5">
      {metrics.map((metric) => (
        <UsageMetric
          key={metric.label}
          icon={metric.icon}
          label={metric.label}
          value={metric.value}
        />
      ))}
    </div>
  )
}

function UsageMetric({ icon, label, value }: { icon: ReactNode; label: string; value: string }) {
  return (
    <div className="min-h-20 rounded-md border border-border bg-background p-3">
      <div className="flex items-center gap-2 text-muted-foreground text-xs">
        {icon}
        <span>{label}</span>
      </div>
      <div className="mt-2 truncate font-semibold text-base">{value}</div>
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
        <div className="flex gap-2">
          <div className="grid grid-rows-7 gap-1 pt-0.5 text-muted-foreground text-[10px] leading-3">
            {['mon', 'tue', 'wed', 'thu', 'fri', 'sat', 'sun'].map((day) => (
              <span key={day} className="h-3">
                {t(`models.usageInsights.weekdays.${day}`)}
              </span>
            ))}
          </div>
          <div>
            <div
              className="grid grid-flow-col grid-rows-7 gap-1"
              style={{ gridTemplateColumns: `repeat(${weekCount}, 0.75rem)` }}
            >
              {slots.map((slot) =>
                slot.entry ? (
                  <button
                    aria-label={t('models.usageInsights.dailyPoint', {
                      date: slot.date,
                      tokens: formatTokenCount(t, slot.entry.tokens),
                    })}
                    className={`size-3 rounded-[3px] border border-border/30 ${HEAT_CLASSES[slot.entry.level]}`}
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
                  <span aria-hidden="true" className="size-3" key={slot.date} />
                ),
              )}
            </div>
            <div
              className="mt-2 grid text-muted-foreground text-[10px]"
              style={{ gridTemplateColumns: `repeat(${weekCount}, 0.75rem)` }}
            >
              {data.monthLabels.map((label) => (
                <span
                  className="min-w-8"
                  key={`${label.date}-${label.label}`}
                  style={{ gridColumn: `${monthLabelColumn(slots[0]?.date, label.date)} / span 4` }}
                >
                  {formatMonthLabel(label.date, i18n.language)}
                </span>
              ))}
            </div>
          </div>
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
