import { Plus } from 'lucide-react'
import { useMemo, useState } from 'react'
import { useTranslation } from 'react-i18next'

import { Button } from '@/shared/ui/button'

import { ModelConfigDialog } from './ModelConfigDialog'
import { ModelDetailsDrawer } from './ModelDetailsDrawer'
import { ModelMatrix } from './ModelMatrix'
import { ModelSummaryBand } from './ModelSummaryBand'
import { useModelSettingsViewModel } from './model-settings-queries'
import { isFailingConnectivity, type ModelAssetRow } from './model-settings-view-model'

type HealthFilter = 'all' | 'online' | 'failing' | 'never_checked' | 'unavailable'

export function ModelSettingsPage() {
  const { t } = useTranslation('settings')
  const {
    isProbePending,
    isQuotaRefreshPending,
    pageState,
    probeConfig,
    refreshQuota,
    refetchAll,
  } = useModelSettingsViewModel()
  const [providerFilter, setProviderFilter] = useState('all')
  const [healthFilter, setHealthFilter] = useState<HealthFilter>('all')
  const [defaultOnly, setDefaultOnly] = useState(false)
  const [failingOnly, setFailingOnly] = useState(false)
  const [search, setSearch] = useState('')
  const [detailsConfigId, setDetailsConfigId] = useState<string | null>(null)
  const [editConfigId, setEditConfigId] = useState<string | null>(null)
  const [createConfigOpen, setCreateConfigOpen] = useState(false)

  const filteredRows = useMemo(() => {
    if (pageState.kind !== 'ready') {
      return []
    }

    return pageState.viewModel.rows.filter((row) =>
      matchesFilters(row, {
        defaultOnly,
        failingOnly,
        healthFilter,
        providerFilter,
        search,
      }),
    )
  }, [defaultOnly, failingOnly, healthFilter, pageState, providerFilter, search])

  if (pageState.kind === 'loading') {
    return (
      <section aria-busy="true" className="space-y-4" data-testid="model-settings-page">
        <h1 className="font-semibold text-xl">{t('models.title')}</h1>
        <div className="rounded-md border border-border bg-surface p-5 text-muted-foreground text-sm">
          {t('models.loading')}
        </div>
      </section>
    )
  }

  if (pageState.kind === 'error') {
    return (
      <section className="space-y-4" data-testid="model-settings-page">
        <h1 className="font-semibold text-xl">{t('models.title')}</h1>
        <div className="rounded-md border border-destructive/30 bg-surface p-5" role="alert">
          <h2 className="font-medium text-destructive text-sm">{t('models.error.title')}</h2>
          <p className="mt-2 text-muted-foreground text-sm">{pageState.safeMessage}</p>
          <Button
            className="mt-4"
            onClick={() => void refetchAll()}
            type="button"
            variant="outline"
          >
            {t('models.error.retry')}
          </Button>
        </div>
      </section>
    )
  }

  const providerOptions = buildProviderOptions(pageState.viewModel.rows)
  const detailsRow =
    pageState.viewModel.rows.find((row) => row.configId === detailsConfigId) ?? null
  const editProfile =
    pageState.viewModel.configs.find((config) => config.id === editConfigId) ?? null

  return (
    <section className="space-y-4" data-testid="model-settings-page">
      <div className="flex flex-wrap items-center justify-between gap-3">
        <h1 className="font-semibold text-xl">{t('models.title')}</h1>
        <Button onClick={() => setCreateConfigOpen(true)} type="button">
          <Plus aria-hidden="true" className="size-4" data-icon />
          {t('provider.newConfig')}
        </Button>
      </div>

      <ModelSummaryBand summary={pageState.viewModel.summary} />

      <search
        aria-label={t('models.filters.label')}
        className="flex flex-wrap items-end gap-3 rounded-md border border-border bg-surface p-3"
      >
        <label className="grid gap-1 text-sm">
          <span className="font-medium text-xs">{t('models.filters.provider')}</span>
          <select
            className="h-9 min-w-40 rounded-sm border border-input bg-background px-2 text-sm outline-none focus-visible:ring-2 focus-visible:ring-ring"
            onChange={(event) => setProviderFilter(event.target.value)}
            value={providerFilter}
          >
            <option value="all">{t('models.filters.allProviders')}</option>
            {providerOptions.map((provider) => (
              <option key={provider.id} value={provider.id}>
                {provider.label}
              </option>
            ))}
          </select>
        </label>
        <label className="grid gap-1 text-sm">
          <span className="font-medium text-xs">{t('models.filters.health')}</span>
          <select
            className="h-9 min-w-36 rounded-sm border border-input bg-background px-2 text-sm outline-none focus-visible:ring-2 focus-visible:ring-ring"
            onChange={(event) => setHealthFilter(event.target.value as HealthFilter)}
            value={healthFilter}
          >
            <option value="all">{t('models.filters.allHealth')}</option>
            <option value="online">{t('models.connectivity.online')}</option>
            <option value="failing">{t('models.filters.failing')}</option>
            <option value="never_checked">{t('models.connectivity.neverChecked')}</option>
            <option value="unavailable">{t('models.unavailable')}</option>
          </select>
        </label>
        <label className="grid min-w-52 flex-1 gap-1 text-sm">
          <span className="font-medium text-xs">{t('models.filters.search')}</span>
          <input
            className="h-9 rounded-sm border border-input bg-background px-2 text-sm outline-none focus-visible:ring-2 focus-visible:ring-ring"
            onChange={(event) => setSearch(event.target.value)}
            placeholder={t('models.filters.searchPlaceholder')}
            type="search"
            value={search}
          />
        </label>
        <label className="flex h-9 items-center gap-2 text-sm">
          <input
            checked={defaultOnly}
            className="size-4 accent-primary"
            onChange={(event) => setDefaultOnly(event.target.checked)}
            type="checkbox"
          />
          {t('models.filters.defaultOnly')}
        </label>
        <label className="flex h-9 items-center gap-2 text-sm">
          <input
            checked={failingOnly}
            className="size-4 accent-primary"
            onChange={(event) => setFailingOnly(event.target.checked)}
            type="checkbox"
          />
          {t('models.filters.failingOnly')}
        </label>
      </search>

      <ModelMatrix
        isProbePending={isProbePending}
        isQuotaRefreshPending={isQuotaRefreshPending}
        onDetails={setDetailsConfigId}
        onEdit={setEditConfigId}
        onProbe={(configId) => {
          void probeConfig(configId, 10_000).catch(() => undefined)
        }}
        onRefreshQuota={(configId) => {
          void refreshQuota(configId).catch(() => undefined)
        }}
        rows={filteredRows}
      />

      <ModelDetailsDrawer
        onEdit={(row) => {
          setDetailsConfigId(null)
          setEditConfigId(row.configId)
        }}
        onOpenChange={(open) => {
          if (!open) {
            setDetailsConfigId(null)
          }
        }}
        open={detailsRow !== null}
        row={detailsRow}
      />
      <ModelConfigDialog
        catalog={pageState.viewModel.catalog}
        onOpenChange={(open) => {
          if (!open) {
            setEditConfigId(null)
            setCreateConfigOpen(false)
          }
        }}
        onSaved={() => {
          void refetchAll()
        }}
        open={createConfigOpen || editProfile !== null}
        profile={editProfile}
      />
    </section>
  )
}

function buildProviderOptions(rows: ModelAssetRow[]) {
  const providers = new Map<string, string>()
  for (const row of rows) {
    providers.set(row.providerId, row.providerDisplayName)
  }
  return [...providers.entries()]
    .map(([id, label]) => ({ id, label }))
    .sort((left, right) => left.label.localeCompare(right.label))
}

function matchesFilters(
  row: ModelAssetRow,
  filters: {
    defaultOnly: boolean
    failingOnly: boolean
    healthFilter: HealthFilter
    providerFilter: string
    search: string
  },
) {
  if (filters.providerFilter !== 'all' && row.providerId !== filters.providerFilter) {
    return false
  }
  if (filters.defaultOnly && !row.isDefault) {
    return false
  }
  if (filters.failingOnly && !isFailingConnectivity(row.connectivity)) {
    return false
  }
  if (filters.healthFilter === 'online' && row.connectivity.status !== 'online') {
    return false
  }
  if (filters.healthFilter === 'failing' && !isFailingConnectivity(row.connectivity)) {
    return false
  }
  if (
    filters.healthFilter !== 'all' &&
    filters.healthFilter !== 'online' &&
    filters.healthFilter !== 'failing' &&
    row.connectivity.status !== filters.healthFilter
  ) {
    return false
  }

  const search = filters.search.trim().toLowerCase()
  if (!search) {
    return true
  }

  return [row.displayName, row.providerDisplayName, row.providerId, row.modelId].some((value) =>
    value.toLowerCase().includes(search),
  )
}
