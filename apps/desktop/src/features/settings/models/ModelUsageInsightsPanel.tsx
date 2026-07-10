import { useMemo, useState } from 'react'
import { useTranslation } from 'react-i18next'
import type { TooltipContentProps } from 'recharts'
import {
  Area,
  AreaChart,
  Bar,
  BarChart,
  CartesianGrid,
  Tooltip as RechartsTooltip,
  ResponsiveContainer,
  XAxis,
  YAxis,
} from 'recharts'

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
        <div className="mb-3 flex flex-wrap items-center justify-between gap-3">
          <h2 className="font-medium text-sm">{t('models.usageInsights.activityTitle')}</h2>
          <div
            aria-label={t('models.usageInsights.views.label')}
            className="inline-flex h-9 items-center justify-center gap-4 text-muted-foreground"
            role="tablist"
          >
            {views.map((view, index) => (
              <button
                aria-controls={`model-usage-${view}-panel`}
                aria-selected={mode === view}
                className={`inline-flex min-h-9 items-center justify-center whitespace-nowrap border-b-2 px-1 text-sm outline-none transition-colors duration-200 focus-visible:ring-2 focus-visible:ring-ring ${
                  mode === view
                    ? 'border-foreground font-semibold text-foreground'
                    : 'border-transparent font-normal hover:text-foreground/90'
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
    <dl className="grid grid-cols-2 overflow-hidden rounded-2xl border border-border/80 @min-[640px]:grid-cols-5">
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
        index >= 2 ? 'border-border/70 border-t @min-[640px]:border-t-0' : ''
      } ${index % 2 === 1 ? 'border-border/70 border-l @min-[640px]:border-l-0' : ''} ${
        index === 4 ? 'col-span-2 @min-[640px]:col-span-1' : ''
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
  const initialActiveIndex = latestActivityIndex(data.daily)
  const [activeIndex, setActiveIndex] = useState(initialActiveIndex)
  const safeActiveIndex = Math.min(activeIndex, Math.max(0, data.daily.length - 1))
  const activeEntry = data.daily[safeActiveIndex]
  const entryIndexByDate = useMemo(
    () => new Map(data.daily.map((entry, index) => [entry.date, index])),
    [data.daily],
  )
  const gridColumns = `repeat(${weekCount}, minmax(10px, 1fr))`
  const minGridWidth = weekCount * 10 + Math.max(0, weekCount - 1) * 3

  if (!data.daily.some((entry) => entry.tokens > 0)) {
    return <EmptyChartState />
  }

  return (
    <div className="overflow-x-auto pb-1">
      <div className="w-full" style={{ minWidth: `${minGridWidth}px` }}>
        <input
          aria-label={t('models.usageInsights.views.daily')}
          aria-valuetext={
            activeEntry
              ? t('models.usageInsights.dailyPoint', {
                  date: activeEntry.date,
                  tokens: formatTokenCount(t, activeEntry.tokens),
                })
              : undefined
          }
          className="peer sr-only"
          data-testid="usage-heatmap-control"
          max={Math.max(0, data.daily.length - 1)}
          min={0}
          onChange={(event) => setActiveIndex(Number(event.currentTarget.value))}
          type="range"
          value={safeActiveIndex}
        />
        <div
          aria-hidden="true"
          className="grid grid-flow-col grid-rows-7 gap-[3px] rounded-sm outline-none peer-focus-visible:ring-2 peer-focus-visible:ring-ring peer-focus-visible:ring-offset-2 peer-focus-visible:ring-offset-surface"
          data-testid="usage-heatmap-grid"
          style={{ gridTemplateColumns: gridColumns }}
        >
          {slots.map((slot) => {
            if (!slot.entry) {
              return (
                <span
                  aria-hidden="true"
                  className="aspect-square w-full rounded-[3px]"
                  key={slot.date}
                />
              )
            }
            const entryIndex = entryIndexByDate.get(slot.date) ?? 0
            const label = t('models.usageInsights.dailyPoint', {
              date: slot.date,
              tokens: formatTokenCount(t, slot.entry.tokens),
            })
            const isActive = entryIndex === safeActiveIndex
            return (
              <span
                aria-hidden="true"
                className={`aspect-square w-full rounded-[3px] transition-[outline-color,transform] duration-100 ${HEAT_CLASSES[slot.entry.level]} ${
                  isActive ? 'outline-2 outline-offset-1 outline-ring' : 'outline-transparent'
                }`}
                data-active={isActive}
                data-level={slot.entry.level}
                data-testid={`usage-day-${slot.date}`}
                key={slot.date}
                onMouseEnter={() => setActiveIndex(entryIndex)}
                title={label}
              />
            )
          })}
        </div>
        <div
          className="mt-2.5 grid gap-[3px] text-muted-foreground text-xs"
          style={{ gridTemplateColumns: gridColumns }}
        >
          {data.monthLabels.map((label) => (
            <span
              className="whitespace-nowrap"
              data-testid="usage-month-label"
              key={`${label.date}-${label.label}`}
              style={{
                gridColumnStart: monthLabelColumn(slots[0]?.date, label.date),
                gridRow: '1',
              }}
            >
              {formatMonthLabel(label.date, i18n.language)}
            </span>
          ))}
        </div>
        <div className="mt-3 flex min-h-5 items-center justify-between gap-4 text-xs">
          <output className="truncate text-muted-foreground">
            {activeEntry
              ? t('models.usageInsights.dailyPoint', {
                  date: activeEntry.date,
                  tokens: formatTokenCount(t, activeEntry.tokens),
                })
              : null}
          </output>
          <div className="flex shrink-0 items-center gap-1.5 text-muted-foreground">
            <span>{t('models.usageInsights.legendLess')}</span>
            {HEAT_CLASSES.map((heatClass) => (
              <span
                aria-hidden="true"
                className={`size-2.5 rounded-[3px] ${heatClass}`}
                key={heatClass}
              />
            ))}
            <span>{t('models.usageInsights.legendMore')}</span>
          </div>
        </div>
      </div>
    </div>
  )
}

function WeeklyTokenChart({ data }: { data: UsageInsightsData }) {
  const { i18n, t } = useTranslation('settings')

  if (!data.weekly.some((week) => week.tokens > 0)) {
    return <EmptyChartState />
  }

  return (
    <figure
      aria-label={t('models.usageInsights.charts.weeklyLabel')}
      data-testid="weekly-token-chart"
    >
      <AccessibleChartData
        labels={data.weekly.map((week) => ({
          label: t('models.usageInsights.weeklyPoint', {
            start: week.weekStart,
            end: week.weekEnd,
            tokens: formatTokenCount(t, week.tokens),
          }),
          value: week.tokens,
        }))}
      />
      <ResponsiveContainer
        className="h-48"
        height={192}
        initialDimension={{ width: 720, height: 192 }}
        minWidth={0}
        width="100%"
      >
        <BarChart accessibilityLayer data={data.weekly} margin={CHART_MARGIN}>
          <CartesianGrid stroke="var(--border)" strokeOpacity={0.7} vertical={false} />
          <XAxis
            axisLine={false}
            dataKey="weekStart"
            minTickGap={36}
            tick={AXIS_TICK}
            tickFormatter={(date) => formatAxisDate(String(date), i18n.language)}
            tickLine={false}
          />
          <YAxis
            axisLine={false}
            tick={AXIS_TICK}
            tickFormatter={(value) => formatCompactNumber(Number(value), i18n.language)}
            tickLine={false}
            width={48}
          />
          <RechartsTooltip
            content={<UsageChartTooltip mode="weekly" />}
            cursor={{ fill: 'var(--muted)', fillOpacity: 0.6 }}
          />
          <Bar
            dataKey="tokens"
            fill="var(--info)"
            isAnimationActive={false}
            maxBarSize={10}
            radius={[3, 3, 0, 0]}
          />
        </BarChart>
      </ResponsiveContainer>
    </figure>
  )
}

function CumulativeTokenChart({ data }: { data: UsageInsightsData }) {
  const { i18n, t } = useTranslation('settings')

  if (!data.cumulative.some((point) => point.tokens > 0)) {
    return <EmptyChartState />
  }

  return (
    <figure
      aria-label={t('models.usageInsights.charts.cumulativeLabel')}
      data-testid="cumulative-token-chart"
    >
      <AccessibleChartData
        labels={data.cumulative.map((point) => ({
          label: t('models.usageInsights.cumulativePoint', {
            date: point.date,
            tokens: formatTokenCount(t, point.tokens),
          }),
          value: point.tokens,
        }))}
      />
      <ResponsiveContainer
        className="h-48"
        height={192}
        initialDimension={{ width: 720, height: 192 }}
        minWidth={0}
        width="100%"
      >
        <AreaChart accessibilityLayer data={data.cumulative} margin={CHART_MARGIN}>
          <CartesianGrid stroke="var(--border)" strokeOpacity={0.7} vertical={false} />
          <XAxis
            axisLine={false}
            dataKey="date"
            minTickGap={44}
            tick={AXIS_TICK}
            tickFormatter={(date) => formatAxisDate(String(date), i18n.language)}
            tickLine={false}
          />
          <YAxis
            axisLine={false}
            tick={AXIS_TICK}
            tickFormatter={(value) => formatCompactNumber(Number(value), i18n.language)}
            tickLine={false}
            width={48}
          />
          <RechartsTooltip content={<UsageChartTooltip mode="cumulative" />} />
          <Area
            activeDot={{ fill: 'var(--surface)', r: 4, stroke: 'var(--info)', strokeWidth: 2 }}
            dataKey="tokens"
            dot={false}
            fill="var(--info)"
            fillOpacity={0.14}
            isAnimationActive={false}
            stroke="var(--info)"
            strokeWidth={2.5}
            type="monotone"
          />
        </AreaChart>
      </ResponsiveContainer>
    </figure>
  )
}

const CHART_MARGIN = { top: 8, right: 8, bottom: 0, left: 0 }
const AXIS_TICK = { fill: 'var(--muted-foreground)', fontSize: 11 }

function AccessibleChartData({ labels }: { labels: { label: string; value: number }[] }) {
  return (
    <ul className="sr-only">
      {labels.map((item) => (
        <li data-token-value={item.value} key={item.label}>
          {item.label}
        </li>
      ))}
    </ul>
  )
}

function UsageChartTooltip({
  active,
  mode,
  payload,
}: Partial<TooltipContentProps<number, string>> & { mode: 'weekly' | 'cumulative' }) {
  const { t } = useTranslation('settings')
  const datum = payload?.[0]?.payload as
    | UsageInsightsData['weekly'][number]
    | UsageInsightsData['cumulative'][number]
    | undefined
  if (!active || !datum) {
    return null
  }
  const label =
    mode === 'weekly' && 'weekStart' in datum
      ? t('models.usageInsights.weeklyPoint', {
          start: datum.weekStart,
          end: datum.weekEnd,
          tokens: formatTokenCount(t, datum.tokens),
        })
      : t('models.usageInsights.cumulativePoint', {
          date: 'date' in datum ? datum.date : '',
          tokens: formatTokenCount(t, datum.tokens),
        })

  return (
    <div className="rounded-md border border-border bg-surface px-3 py-2 text-foreground text-xs shadow-md">
      {label}
    </div>
  )
}

function EmptyChartState() {
  const { t } = useTranslation('settings')
  return (
    <div className="flex h-48 items-center justify-center border-border border-b text-muted-foreground text-sm">
      {t('models.usageInsights.empty')}
    </div>
  )
}

function latestActivityIndex(daily: UsageInsightsData['daily']): number {
  for (let index = daily.length - 1; index >= 0; index -= 1) {
    if (daily[index].tokens > 0) {
      return index
    }
  }
  return 0
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
  return new Intl.DateTimeFormat(locale, {
    month: 'short',
    timeZone: 'UTC',
  }).format(new Date(`${date}T00:00:00Z`))
}

function formatAxisDate(date: string, locale: string): string {
  return new Intl.DateTimeFormat(locale, {
    day: 'numeric',
    month: 'short',
    timeZone: 'UTC',
  }).format(new Date(`${date}T00:00:00Z`))
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
