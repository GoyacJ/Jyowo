import type {
  GetModelUsageSummaryResponse,
  ListOfficialQuotaSnapshotsResponse,
  ListProviderCapabilityRouteOptionsResponse,
  ListProviderCapabilityRoutesResponse,
  ListProviderProbeSnapshotsResponse,
  ListProviderSettingsResponse,
  ModelProviderCatalogResponse,
  ProviderConfig,
} from '@/shared/tauri/commands'

type ProviderProbeSnapshot = ListProviderProbeSnapshotsResponse['snapshots'][number]
type OfficialQuotaSnapshot = ListOfficialQuotaSnapshotsResponse['snapshots'][number]
type ProviderCapabilityRoute = ListProviderCapabilityRoutesResponse['routes'][number]
type ProviderCapabilityRouteOption = ListProviderCapabilityRouteOptionsResponse['options'][number]
type CapabilityRouteKind = ProviderCapabilityRouteOption['kind']
type OfficialQuotaScope = OfficialQuotaSnapshot['scope']
type UsageSnapshot = GetModelUsageSummaryResponse['today']['total']

export type QuerySlice<T> =
  | { status: 'loading' }
  | { status: 'idle' }
  | { status: 'error'; safeMessage: string }
  | { status: 'ready'; data: T }

type SectionState<T> = { status: 'unavailable' } | { status: 'ready'; data: T }

export type ModelSettingsQueryInputs = {
  catalog: QuerySlice<ModelProviderCatalogResponse>
  providerSettings: QuerySlice<ListProviderSettingsResponse>
  probeSnapshots: QuerySlice<ListProviderProbeSnapshotsResponse>
  usageSummary: QuerySlice<GetModelUsageSummaryResponse>
  quotaSnapshots: QuerySlice<ListOfficialQuotaSnapshotsResponse>
  routes: QuerySlice<ListProviderCapabilityRoutesResponse>
  routeOptions: QuerySlice<ListProviderCapabilityRouteOptionsResponse>
}

type ConnectivityDisplayState =
  | { status: 'never_checked' }
  | { status: 'unavailable' }
  | {
      status: Exclude<ProviderProbeSnapshot['status'], never>
      checkedAt: string
      latencyMs?: number
      timeoutMs: number
      errorKind?: ProviderProbeSnapshot['errorKind']
      safeMessage?: string
    }

type UsageDisplayState =
  | { status: 'unavailable' }
  | {
      status: 'ready'
      sharedModelUsage: boolean
      today: UsageSnapshot
      monthToDate: UsageSnapshot
      allTime: UsageSnapshot
    }

type QuotaDisplayState =
  | { status: 'unavailable' }
  | ({
      scope: OfficialQuotaScope
      scopeLabel: OfficialQuotaScope
      sourceUrl: string
      fetchedAt: string
      expiresAt: string
      isStale: boolean
      safeMessage?: string
      billingLabel?: string
      quotaUsed?: number
      quotaTotal?: number
      quotaRemaining?: number
      unit?: string
    } & Pick<OfficialQuotaSnapshot, 'status'>)

type ModelAssetRow = {
  configId: string
  providerId: ProviderConfig['providerId']
  modelId: string
  displayName: string
  providerDisplayName: string
  isDefault: boolean
  hasApiKey: boolean
  connectivity: ConnectivityDisplayState
  usage: UsageDisplayState
  quota: QuotaDisplayState
}

type ModelSettingsSummaryMetric<T> = SectionState<T>

type ModelSettingsSummaryView = {
  defaultModel: ModelSettingsSummaryMetric<{
    configId: string
    displayName: string
    providerDisplayName: string
  }>
  configuredModels: ModelSettingsSummaryMetric<{
    total: number
    available: number
    failing: number
  }>
  localUsage: ModelSettingsSummaryMetric<{
    today: UsageSnapshot
    monthToDate: UsageSnapshot
    allTime: UsageSnapshot
  }>
  officialQuota: ModelSettingsSummaryMetric<{
    configuredProfiles: number
    supported: number
    unsupported: number
    authRequired: number
    failed: number
  }>
}

type CapabilityRouteUnavailableTarget = {
  configId: string
  providerId: string
  operationId: string
  reason: string
}

type CapabilityRouteRow = {
  kind: CapabilityRouteKind
  savedRoute: ProviderCapabilityRoute | null
  eligibleTargetCount: number
  unavailableTargets: CapabilityRouteUnavailableTarget[]
}

type ModelSettingsViewModel = {
  summary: ModelSettingsSummaryView
  rows: ModelAssetRow[]
  capabilityRoutes: SectionState<CapabilityRouteRow[]>
}

export type ModelSettingsPageState =
  | { kind: 'loading' }
  | { kind: 'error'; safeMessage: string }
  | { kind: 'ready'; viewModel: ModelSettingsViewModel }

const FAILING_PROBE_STATUSES = new Set<ProviderProbeSnapshot['status']>([
  'timeout',
  'unauthenticated',
  'rate_limited',
  'failed',
])

const capabilityRouteKindOrder = [
  'image_generation',
  'video_generation',
  'text_to_speech',
  'speech_to_text',
  'music_generation',
] as const satisfies readonly CapabilityRouteKind[]

const ZERO_USAGE: UsageSnapshot = {
  cacheReadTokens: 0,
  cacheWriteTokens: 0,
  costMicros: 0,
  inputTokens: 0,
  outputTokens: 0,
  toolCalls: 0,
}

export function modelUsageKey(providerId: string, modelId: string): string {
  return `${providerId}/${modelId}`
}

export function buildModelSettingsPageState(
  input: ModelSettingsQueryInputs,
): ModelSettingsPageState {
  if (isCriticalQueryLoading(input)) {
    return { kind: 'loading' }
  }

  const settingsError =
    input.providerSettings.status === 'error' ? input.providerSettings.safeMessage : null
  const catalogError = input.catalog.status === 'error' ? input.catalog.safeMessage : null

  if (settingsError || catalogError) {
    return { kind: 'error', safeMessage: settingsError ?? catalogError ?? 'Model settings failed' }
  }

  if (input.providerSettings.status !== 'ready' || input.catalog.status !== 'ready') {
    return { kind: 'loading' }
  }

  return {
    kind: 'ready',
    viewModel: buildModelSettingsViewModel(input),
  }
}

export function buildModelSettingsViewModel(
  input: ModelSettingsQueryInputs,
): ModelSettingsViewModel {
  const settings = input.providerSettings.status === 'ready' ? input.providerSettings.data : null
  const catalog = input.catalog.status === 'ready' ? input.catalog.data : null

  if (!settings || !catalog) {
    return emptyModelSettingsViewModel()
  }

  const providerDisplayNames = new Map(
    catalog.providers.map((provider) => [provider.providerId, provider.displayName]),
  )
  const probeAvailable = input.probeSnapshots.status === 'ready'
  const quotaAvailable = input.quotaSnapshots.status === 'ready'
  const usageAvailable = input.usageSummary.status === 'ready'
  const probeByConfigId = buildProbeIndex(input.probeSnapshots)
  const quotaByConfigId = buildQuotaIndex(input.quotaSnapshots)
  const usageLookup = buildUsageLookup(input.usageSummary)
  const sharedUsageKeys = buildSharedUsageKeys(settings.configs)

  const rows = settings.configs.map((config) =>
    buildModelAssetRow({
      config,
      providerDisplayName: providerDisplayNames.get(config.providerId) ?? config.providerId,
      probe: probeByConfigId.get(config.id),
      quota: quotaByConfigId.get(config.id),
      probeAvailable,
      quotaAvailable,
      usageAvailable,
      usageLookup,
      sharedUsageKeys,
    }),
  )

  return {
    summary: buildModelSettingsSummary({
      rows,
      settings,
      usageSummary: input.usageSummary,
      quotaSnapshots: input.quotaSnapshots,
    }),
    rows,
    capabilityRoutes: buildCapabilityRoutesSection(input.routes, input.routeOptions, settings),
  }
}

function emptyModelSettingsViewModel(): ModelSettingsViewModel {
  return {
    summary: {
      defaultModel: { status: 'unavailable' },
      configuredModels: { status: 'unavailable' },
      localUsage: { status: 'unavailable' },
      officialQuota: { status: 'unavailable' },
    },
    rows: [],
    capabilityRoutes: { status: 'unavailable' },
  }
}

function isCriticalQueryLoading(input: ModelSettingsQueryInputs): boolean {
  return (
    input.providerSettings.status === 'loading' ||
    input.providerSettings.status === 'idle' ||
    input.catalog.status === 'loading' ||
    input.catalog.status === 'idle'
  )
}

function buildProbeIndex(
  slice: QuerySlice<ListProviderProbeSnapshotsResponse>,
): Map<string, ProviderProbeSnapshot> {
  if (slice.status !== 'ready') {
    return new Map()
  }

  return new Map(slice.data.snapshots.map((snapshot) => [snapshot.configId, snapshot]))
}

function buildQuotaIndex(
  slice: QuerySlice<ListOfficialQuotaSnapshotsResponse>,
): Map<string, OfficialQuotaSnapshot> {
  if (slice.status !== 'ready') {
    return new Map()
  }

  return new Map(slice.data.snapshots.map((snapshot) => [snapshot.configId, snapshot]))
}

function buildUsageLookup(
  slice: QuerySlice<GetModelUsageSummaryResponse>,
): Map<string, { today: UsageSnapshot; monthToDate: UsageSnapshot; allTime: UsageSnapshot }> {
  if (slice.status !== 'ready') {
    return new Map()
  }

  const summary = slice.data
  const keys = new Set<string>([
    ...summary.today.byModel.map((bucket) => modelUsageKey(bucket.providerId, bucket.modelId)),
    ...summary.monthToDate.byModel.map((bucket) =>
      modelUsageKey(bucket.providerId, bucket.modelId),
    ),
    ...summary.allTime.byModel.map((bucket) => modelUsageKey(bucket.providerId, bucket.modelId)),
  ])

  return new Map(
    [...keys].map((key) => {
      const [providerId, modelId] = key.split('/')
      return [
        key,
        {
          today: findUsageInWindow(summary.today, providerId, modelId),
          monthToDate: findUsageInWindow(summary.monthToDate, providerId, modelId),
          allTime: findUsageInWindow(summary.allTime, providerId, modelId),
        },
      ]
    }),
  )
}

function buildSharedUsageKeys(configs: ProviderConfig[]): Set<string> {
  const counts = new Map<string, number>()

  for (const config of configs) {
    const key = modelUsageKey(config.providerId, config.modelId)
    counts.set(key, (counts.get(key) ?? 0) + 1)
  }

  return new Set([...counts.entries()].filter(([, count]) => count > 1).map(([key]) => key))
}

function buildModelAssetRow({
  config,
  providerDisplayName,
  probe,
  quota,
  probeAvailable,
  quotaAvailable,
  usageAvailable,
  usageLookup,
  sharedUsageKeys,
}: {
  config: ProviderConfig
  providerDisplayName: string
  probe: ProviderProbeSnapshot | undefined
  quota: OfficialQuotaSnapshot | undefined
  probeAvailable: boolean
  quotaAvailable: boolean
  usageAvailable: boolean
  usageLookup: Map<
    string,
    { today: UsageSnapshot; monthToDate: UsageSnapshot; allTime: UsageSnapshot }
  >
  sharedUsageKeys: Set<string>
}): ModelAssetRow {
  const usageKey = modelUsageKey(config.providerId, config.modelId)
  const usageValues = usageAvailable
    ? (usageLookup.get(usageKey) ?? {
        today: ZERO_USAGE,
        monthToDate: ZERO_USAGE,
        allTime: ZERO_USAGE,
      })
    : undefined

  return {
    configId: config.id,
    providerId: config.providerId,
    modelId: config.modelId,
    displayName: config.displayName,
    providerDisplayName,
    isDefault: config.isDefault,
    hasApiKey: config.hasApiKey,
    connectivity: buildConnectivityDisplay(probe, probeAvailable),
    usage: buildUsageDisplay(usageValues, sharedUsageKeys.has(usageKey)),
    quota: buildQuotaDisplay(quota, quotaAvailable),
  }
}

function buildConnectivityDisplay(
  probe: ProviderProbeSnapshot | undefined,
  probeAvailable: boolean,
): ConnectivityDisplayState {
  if (!probeAvailable) {
    return { status: 'unavailable' }
  }

  if (!probe) {
    return { status: 'never_checked' }
  }

  return {
    status: probe.status,
    checkedAt: probe.checkedAt,
    latencyMs: probe.latencyMs,
    timeoutMs: probe.timeoutMs,
    errorKind: probe.errorKind,
    safeMessage: probe.safeMessage,
  }
}

function buildUsageDisplay(
  usage: { today: UsageSnapshot; monthToDate: UsageSnapshot; allTime: UsageSnapshot } | undefined,
  sharedModelUsage: boolean,
): UsageDisplayState {
  if (!usage) {
    return { status: 'unavailable' }
  }

  return {
    status: 'ready',
    sharedModelUsage,
    today: usage.today,
    monthToDate: usage.monthToDate,
    allTime: usage.allTime,
  }
}

function buildQuotaDisplay(
  quota: OfficialQuotaSnapshot | undefined,
  quotaAvailable: boolean,
): QuotaDisplayState {
  if (!quotaAvailable) {
    return { status: 'unavailable' }
  }

  if (!quota) {
    return { status: 'unavailable' }
  }

  return {
    status: quota.status,
    scope: quota.scope,
    scopeLabel: quota.scope,
    sourceUrl: quota.sourceUrl,
    fetchedAt: quota.fetchedAt,
    expiresAt: quota.expiresAt,
    isStale: quota.isStale,
    safeMessage: quota.safeMessage,
    billingLabel: quota.billingLabel,
    quotaUsed: quota.quotaUsed,
    quotaTotal: quota.quotaTotal,
    quotaRemaining: quota.quotaRemaining,
    unit: quota.unit,
  }
}

function findUsageInWindow(
  window: GetModelUsageSummaryResponse['today'],
  providerId: string,
  modelId: string,
): UsageSnapshot {
  const bucket = window.byModel.find(
    (entry) => entry.providerId === providerId && entry.modelId === modelId,
  )
  return bucket?.usage ?? ZERO_USAGE
}

function buildModelSettingsSummary({
  rows,
  settings,
  usageSummary,
  quotaSnapshots,
}: {
  rows: ModelAssetRow[]
  settings: ListProviderSettingsResponse
  usageSummary: QuerySlice<GetModelUsageSummaryResponse>
  quotaSnapshots: QuerySlice<ListOfficialQuotaSnapshotsResponse>
}): ModelSettingsSummaryView {
  const defaultConfig = settings.configs.find((config) => config.isDefault) ?? null

  return {
    defaultModel: defaultConfig
      ? {
          status: 'ready',
          data: {
            configId: defaultConfig.id,
            displayName: defaultConfig.displayName,
            providerDisplayName:
              rows.find((row) => row.configId === defaultConfig.id)?.providerDisplayName ??
              defaultConfig.providerId,
          },
        }
      : { status: 'unavailable' },
    configuredModels: {
      status: 'ready',
      data: {
        total: rows.length,
        available: rows.filter((row) => row.connectivity.status === 'online').length,
        failing: rows.filter((row) => isFailingConnectivity(row.connectivity)).length,
      },
    },
    localUsage: buildSummaryLocalUsage(usageSummary),
    officialQuota: buildSummaryOfficialQuota(rows.length, quotaSnapshots),
  }
}

function buildSummaryLocalUsage(
  usageSummary: QuerySlice<GetModelUsageSummaryResponse>,
): ModelSettingsSummaryMetric<{
  today: UsageSnapshot
  monthToDate: UsageSnapshot
  allTime: UsageSnapshot
}> {
  if (usageSummary.status !== 'ready') {
    return { status: 'unavailable' }
  }

  return {
    status: 'ready',
    data: {
      today: usageSummary.data.today.total,
      monthToDate: usageSummary.data.monthToDate.total,
      allTime: usageSummary.data.allTime.total,
    },
  }
}

function buildSummaryOfficialQuota(
  configuredProfiles: number,
  quotaSnapshots: QuerySlice<ListOfficialQuotaSnapshotsResponse>,
): ModelSettingsSummaryMetric<{
  configuredProfiles: number
  supported: number
  unsupported: number
  authRequired: number
  failed: number
}> {
  if (quotaSnapshots.status !== 'ready') {
    return { status: 'unavailable' }
  }

  const snapshots = quotaSnapshots.data.snapshots

  return {
    status: 'ready',
    data: {
      configuredProfiles,
      supported: snapshots.filter((snapshot) => snapshot.status === 'supported').length,
      unsupported: snapshots.filter((snapshot) => snapshot.status === 'unsupported').length,
      authRequired: snapshots.filter((snapshot) => snapshot.status === 'authRequired').length,
      failed: snapshots.filter((snapshot) => snapshot.status === 'failed').length,
    },
  }
}

function buildCapabilityRoutesSection(
  routes: QuerySlice<ListProviderCapabilityRoutesResponse>,
  routeOptions: QuerySlice<ListProviderCapabilityRouteOptionsResponse>,
  settings: ListProviderSettingsResponse,
): SectionState<CapabilityRouteRow[]> {
  if (routes.status === 'error' || routeOptions.status === 'error') {
    return { status: 'unavailable' }
  }

  if (routes.status !== 'ready' || routeOptions.status !== 'ready') {
    return { status: 'unavailable' }
  }

  return {
    status: 'ready',
    data: buildCapabilityRouteRows(routeOptions.data.options, routes.data.routes, settings),
  }
}

function buildCapabilityRouteRows(
  options: ProviderCapabilityRouteOption[],
  routes: ProviderCapabilityRoute[],
  settings: ListProviderSettingsResponse,
): CapabilityRouteRow[] {
  const kinds = new Set<CapabilityRouteKind>([
    ...options.map((option) => option.kind),
    ...routes.map((route) => route.kind),
  ])

  return [...kinds]
    .sort((left, right) => capabilityRouteKindSortValue(left) - capabilityRouteKindSortValue(right))
    .map((kind) => {
      const kindOptions = options.filter((option) => option.kind === kind)
      const unavailableTargets = kindOptions
        .filter((option) => !option.runtimeSupported && option.unavailableReason)
        .map((option) => ({
          configId: option.configId,
          providerId: option.providerId,
          operationId: option.operationId,
          reason: option.unavailableReason ?? 'Unavailable',
        }))
      const eligibleTargetCount = new Set(
        kindOptions
          .filter((option) => option.runtimeSupported)
          .map((option) => `${option.configId}::${option.providerId}`),
      ).size

      return {
        kind,
        savedRoute:
          routes.find(
            (route) =>
              route.kind === kind &&
              route.enabled &&
              settings.configs.some((config) => config.id === route.configId),
          ) ?? null,
        eligibleTargetCount,
        unavailableTargets,
      }
    })
}

function capabilityRouteKindSortValue(kind: CapabilityRouteKind): number {
  const index = capabilityRouteKindOrder.indexOf(kind)
  return index === -1 ? capabilityRouteKindOrder.length : index
}

export function isFailingConnectivity(connectivity: ConnectivityDisplayState): boolean {
  if (connectivity.status === 'never_checked' || connectivity.status === 'unavailable') {
    return false
  }

  return FAILING_PROBE_STATUSES.has(connectivity.status)
}

export function isModelScopedQuota(scope: OfficialQuotaScope): boolean {
  return scope === 'model'
}

export function emptyUsageSummary(): GetModelUsageSummaryResponse {
  return {
    timezoneOffsetMinutes: 0,
    today: { period: 'today', total: ZERO_USAGE, byModel: [] },
    monthToDate: { period: 'month_to_date', total: ZERO_USAGE, byModel: [] },
    allTime: { period: 'all_time', total: ZERO_USAGE, byModel: [] },
    generatedAt: '1970-01-01T00:00:00Z',
  }
}
