import { Plus } from 'lucide-react'
import { useMemo, useState } from 'react'
import { useTranslation } from 'react-i18next'

import { useActiveProjectPath } from '@/features/workspace/use-active-project-path'
import { Badge } from '@/shared/ui/badge'
import { Button } from '@/shared/ui/button'
import { Tabs, TabsContent, TabsList, TabsTrigger } from '@/shared/ui/tabs'

import { CapabilityRouteEditorDrawer } from './CapabilityRouteEditorDrawer'
import { CapabilityRoutesPanel } from './CapabilityRoutesPanel'
import { ModelConfigDialog } from './ModelConfigDialog'
import { ModelDetailsDrawer } from './ModelDetailsDrawer'
import { ModelMatrix } from './ModelMatrix'
import { ModelSummaryBand } from './ModelSummaryBand'
import { useModelSettingsViewModel } from './model-settings-queries'
import {
  type CapabilityRouteRow,
  isFailingConnectivity,
  type ModelAssetRow,
} from './model-settings-view-model'

type HealthFilter = 'all' | 'online' | 'failing' | 'never_checked' | 'unavailable'

export function ModelSettingsPage() {
  const { t } = useTranslation('settings')
  const activeProjectPathQuery = useActiveProjectPath()
  const routeHasProjectScope = activeProjectPathQuery.data != null
  const {
    isAnySetDefaultPending,
    isProbePending,
    isQuotaRefreshPending,
    isSetDefaultPending,
    pageState,
    probeConfig,
    refreshQuota,
    refetchAll,
    deleteCapabilityRoute,
    saveCapabilityRoute,
    setDefaultConfig,
  } = useModelSettingsViewModel()
  const [activeSurface, setActiveSurface] = useState('models')
  const [providerFilter, setProviderFilter] = useState('all')
  const [healthFilter, setHealthFilter] = useState<HealthFilter>('all')
  const [defaultOnly, setDefaultOnly] = useState(false)
  const [failingOnly, setFailingOnly] = useState(false)
  const [search, setSearch] = useState('')
  const [detailsConfigId, setDetailsConfigId] = useState<string | null>(null)
  const [createConfigOpen, setCreateConfigOpen] = useState(false)
  const [routeEditorRoute, setRouteEditorRoute] = useState<CapabilityRouteRow | null>(null)

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
  const scopeLabelKey =
    pageState.viewModel.selectionScope === 'project'
      ? 'scope.projectOverrides'
      : 'scope.globalDefaults'

  return (
    <section className="space-y-4" data-testid="model-settings-page">
      <div className="flex flex-wrap items-center justify-between gap-3">
        <div className="flex flex-wrap items-center gap-2">
          <h1 className="font-semibold text-xl">{t('models.title')}</h1>
          <Badge variant="outline">{t(scopeLabelKey)}</Badge>
        </div>
        <Button onClick={() => setCreateConfigOpen(true)} type="button">
          <Plus aria-hidden="true" className="size-4" data-icon />
          {t('provider.newConfig')}
        </Button>
      </div>

      <Tabs onValueChange={setActiveSurface} value={activeSurface}>
        <TabsList>
          <TabsTrigger onClick={() => setActiveSurface('models')} value="models">
            {t('models.tabs.models')}
          </TabsTrigger>
          <TabsTrigger
            onClick={() => setActiveSurface('capabilityRoutes')}
            value="capabilityRoutes"
          >
            {t('models.tabs.capabilityRoutes')}
          </TabsTrigger>
        </TabsList>

        <TabsContent className="space-y-4" value="models">
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
            isAnySetDefaultPending={isAnySetDefaultPending}
            isProbePending={isProbePending}
            isQuotaRefreshPending={isQuotaRefreshPending}
            isSetDefaultPending={isSetDefaultPending}
            onDetails={setDetailsConfigId}
            onProbe={(configId) => {
              void probeConfig(configId, 10_000).catch(() => undefined)
            }}
            onRefreshQuota={(configId) => {
              void refreshQuota(configId).catch(() => undefined)
            }}
            onSetDefault={(row) => {
              void setDefaultConfig(defaultRequestFromRow(row)).catch(() => undefined)
            }}
            rows={filteredRows}
          />
        </TabsContent>

        <TabsContent value="capabilityRoutes">
          <CapabilityRoutesPanel
            hasProjectScope={routeHasProjectScope}
            onConfigure={setRouteEditorRoute}
            routeSection={pageState.viewModel.capabilityRoutes}
          />
        </TabsContent>
      </Tabs>

      <ModelDetailsDrawer
        catalog={pageState.viewModel.catalog}
        onSaved={async () => {
          await refetchAll()
        }}
        onUseForRoute={(kind) => {
          if (pageState.viewModel.capabilityRoutes.status !== 'ready') {
            return
          }
          const route =
            pageState.viewModel.capabilityRoutes.data.find(
              (candidate) => candidate.kind === kind,
            ) ?? null
          setDetailsConfigId(null)
          setActiveSurface('capabilityRoutes')
          setRouteEditorRoute(route)
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
            setCreateConfigOpen(false)
          }
        }}
        onSaved={() => {
          void refetchAll()
        }}
        open={createConfigOpen}
        profile={null}
      />
      <CapabilityRouteEditorDrawer
        onClear={async (request) => {
          await deleteCapabilityRoute(request)
          setRouteEditorRoute(null)
          await refetchAll()
        }}
        onOpenChange={(open) => {
          if (!open) {
            setRouteEditorRoute(null)
          }
        }}
        onSave={async (route) => {
          await saveCapabilityRoute({ route })
          setRouteEditorRoute(null)
          await refetchAll()
        }}
        open={routeEditorRoute !== null}
        route={routeEditorRoute}
      />
    </section>
  )
}

function defaultRequestFromRow(row: ModelAssetRow) {
  return {
    ...(row.baseUrl ? { baseUrl: row.baseUrl } : {}),
    configId: row.configId,
    displayName: row.displayName,
    modelId: row.modelId,
    providerId: row.providerId,
  }
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
