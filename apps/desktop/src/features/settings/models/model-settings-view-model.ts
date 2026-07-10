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

export type SectionState<T> =
  | { status: 'loading' }
  | { status: 'error'; safeMessage: string }
  | { status: 'unavailable' }
  | { status: 'ready'; data: T }

export type ModelSettingsQueryInputs = {
  catalog: QuerySlice<ModelProviderCatalogResponse>
  providerSettings: QuerySlice<ListProviderSettingsResponse>
  probeSnapshots: QuerySlice<ListProviderProbeSnapshotsResponse>
  usageSummary: QuerySlice<GetModelUsageSummaryResponse>
  quotaSnapshots: QuerySlice<ListOfficialQuotaSnapshotsResponse>
  routes: QuerySlice<ListProviderCapabilityRoutesResponse>
  routeOptions: QuerySlice<ListProviderCapabilityRouteOptionsResponse>
}

export type ConnectivityDisplayState =
  | { status: 'loading' }
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

export type UsageDisplayState =
  | { status: 'loading' }
  | { status: 'unavailable' }
  | {
      status: 'ready'
      sharedModelUsage: boolean
      today: UsageSnapshot
      monthToDate: UsageSnapshot
      allTime: UsageSnapshot
    }

export type QuotaDisplayState =
  | { status: 'loading' }
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

export type ModelAssetRow = {
  configId: string
  baseUrl?: string
  providerId: ProviderConfig['providerId']
  protocol: ProviderConfig['protocol']
  providerDefaults?: ProviderConfig['providerDefaults']
  modelId: string
  modelOptions?: ProviderConfig['modelOptions']
  modelDescriptor?: ProviderConfig['modelDescriptor']
  displayName: string
  providerDisplayName: string
  isDefault: boolean
  hasApiKey: boolean
  hasOfficialQuotaApiKey: boolean
  connectivity: ConnectivityDisplayState
  usage: UsageDisplayState
  quota: QuotaDisplayState
  routeBindings: SectionState<ModelRouteBinding[]>
}

type ModelSettingsSummaryMetric<T> = SectionState<T>

export type ModelSettingsSummaryView = {
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

type ModelRouteBinding = {
  kind: CapabilityRouteKind
  operationIds: string[]
}

type CapabilityRouteTarget = {
  configId: string
  providerId: string
  modelId: string
  displayName: string
  providerDisplayName: string
  operationIds: string[]
  execution: ProviderCapabilityRouteOption['execution']
  costRisk: ProviderCapabilityRouteOption['costRisk']
  health: ConnectivityDisplayState
}

type CapabilityRouteUnavailableTarget = {
  configId: string
  providerId: string
  modelId: string
  displayName: string
  operationId: string
  reason: string
}

export type CapabilityRouteRow = {
  kind: CapabilityRouteKind
  savedRoute: ProviderCapabilityRoute | null
  selectedTarget: CapabilityRouteTarget | null
  eligibleTargets: CapabilityRouteTarget[]
  unavailableTargets: CapabilityRouteUnavailableTarget[]
}

export type ModelSettingsViewModel = {
  summary: ModelSettingsSummaryView
  rows: ModelAssetRow[]
  catalog: ModelProviderCatalogResponse
  configs: ProviderConfig[]
  selectionScope: ListProviderSettingsResponse['selectionScope']
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
  'three_d_generation',
  'embedding_generation',
  'file_operation',
  'speech_to_text',
  'text_to_speech',
  'music_generation',
  'embedding_generation',
  'moderation',
  'file_management',
  'vector_store_management',
  'batch_job',
  'fine_tuning_job',
  'eval_run',
  'container_session',
  'realtime_session',
  'admin_operation',
  'webhook_verification',
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
      probeStatus: input.probeSnapshots.status,
      quotaStatus: input.quotaSnapshots.status,
      usageStatus: input.usageSummary.status,
      usageLookup,
      sharedUsageKeys,
    }),
  )

  const capabilityRoutes = buildCapabilityRoutesSection(
    input.routes,
    input.routeOptions,
    settings,
    rows,
  )
  const rowsWithRouteBindings = attachRouteBindingsToRows(rows, capabilityRoutes)

  return {
    summary: buildModelSettingsSummary({
      rows: rowsWithRouteBindings,
      settings,
      usageSummary: input.usageSummary,
      quotaSnapshots: input.quotaSnapshots,
    }),
    rows: rowsWithRouteBindings,
    catalog,
    configs: settings.configs,
    selectionScope: settings.selectionScope,
    capabilityRoutes,
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
    catalog: { providers: [] },
    configs: [],
    selectionScope: 'global',
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
  probeStatus,
  quotaStatus,
  usageStatus,
  usageLookup,
  sharedUsageKeys,
}: {
  config: ProviderConfig
  providerDisplayName: string
  probe: ProviderProbeSnapshot | undefined
  quota: OfficialQuotaSnapshot | undefined
  probeStatus: QuerySlice<ListProviderProbeSnapshotsResponse>['status']
  quotaStatus: QuerySlice<ListOfficialQuotaSnapshotsResponse>['status']
  usageStatus: QuerySlice<GetModelUsageSummaryResponse>['status']
  usageLookup: Map<
    string,
    { today: UsageSnapshot; monthToDate: UsageSnapshot; allTime: UsageSnapshot }
  >
  sharedUsageKeys: Set<string>
}): ModelAssetRow {
  const usageKey = modelUsageKey(config.providerId, config.modelId)
  const usageValues =
    usageStatus === 'ready'
      ? (usageLookup.get(usageKey) ?? {
          today: ZERO_USAGE,
          monthToDate: ZERO_USAGE,
          allTime: ZERO_USAGE,
        })
      : undefined

  return {
    configId: config.id,
    baseUrl: config.baseUrl,
    providerId: config.providerId,
    protocol: config.protocol,
    providerDefaults: config.providerDefaults,
    modelId: config.modelId,
    modelOptions: config.modelOptions,
    modelDescriptor: config.modelDescriptor,
    displayName: config.displayName,
    providerDisplayName,
    isDefault: config.isDefault,
    hasApiKey: config.hasApiKey,
    hasOfficialQuotaApiKey: config.hasOfficialQuotaApiKey,
    connectivity: buildConnectivityDisplay(probe, probeStatus),
    usage: buildUsageDisplay(usageValues, sharedUsageKeys.has(usageKey), usageStatus),
    quota: buildQuotaDisplay(quota, quotaStatus),
    routeBindings: { status: 'loading' },
  }
}

function buildConnectivityDisplay(
  probe: ProviderProbeSnapshot | undefined,
  probeStatus: QuerySlice<ListProviderProbeSnapshotsResponse>['status'],
): ConnectivityDisplayState {
  if (isQuerySliceLoading(probeStatus)) {
    return { status: 'loading' }
  }

  if (probeStatus !== 'ready') {
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
  usageStatus: QuerySlice<GetModelUsageSummaryResponse>['status'],
): UsageDisplayState {
  if (isQuerySliceLoading(usageStatus)) {
    return { status: 'loading' }
  }

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
  quotaStatus: QuerySlice<ListOfficialQuotaSnapshotsResponse>['status'],
): QuotaDisplayState {
  if (isQuerySliceLoading(quotaStatus)) {
    return { status: 'loading' }
  }

  if (quotaStatus !== 'ready') {
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
    configuredModels: buildSummaryConfiguredModels(rows),
    localUsage: buildSummaryLocalUsage(usageSummary),
    officialQuota: buildSummaryOfficialQuota(rows.length, quotaSnapshots),
  }
}

function buildSummaryConfiguredModels(rows: ModelAssetRow[]): ModelSettingsSummaryMetric<{
  total: number
  available: number
  failing: number
}> {
  if (rows.some((row) => row.connectivity.status === 'loading')) {
    return { status: 'loading' }
  }

  if (rows.some((row) => row.connectivity.status === 'unavailable')) {
    return { status: 'unavailable' }
  }

  return {
    status: 'ready',
    data: {
      total: rows.length,
      available: rows.filter((row) => row.connectivity.status === 'online').length,
      failing: rows.filter((row) => isFailingConnectivity(row.connectivity)).length,
    },
  }
}

function buildSummaryLocalUsage(
  usageSummary: QuerySlice<GetModelUsageSummaryResponse>,
): ModelSettingsSummaryMetric<{
  today: UsageSnapshot
  monthToDate: UsageSnapshot
  allTime: UsageSnapshot
}> {
  if (isQuerySliceLoading(usageSummary.status)) {
    return { status: 'loading' }
  }

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
  if (isQuerySliceLoading(quotaSnapshots.status)) {
    return { status: 'loading' }
  }

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
  rows: ModelAssetRow[],
): SectionState<CapabilityRouteRow[]> {
  if (routes.status === 'error' || routeOptions.status === 'error') {
    return {
      status: 'error',
      safeMessage:
        routes.status === 'error'
          ? routes.safeMessage
          : routeOptions.status === 'error'
            ? routeOptions.safeMessage
            : 'Capability routes unavailable',
    }
  }

  if (routes.status !== 'ready' || routeOptions.status !== 'ready') {
    return { status: 'loading' }
  }

  return {
    status: 'ready',
    data: buildCapabilityRouteRows(routeOptions.data.options, routes.data.routes, settings, rows),
  }
}

function buildCapabilityRouteRows(
  options: ProviderCapabilityRouteOption[],
  routes: ProviderCapabilityRoute[],
  settings: ListProviderSettingsResponse,
  rows: ModelAssetRow[],
): CapabilityRouteRow[] {
  const kinds = new Set<CapabilityRouteKind>([
    ...options.map((option) => option.kind),
    ...routes.map((route) => route.kind),
  ])

  return [...kinds]
    .sort((left, right) => capabilityRouteKindSortValue(left) - capabilityRouteKindSortValue(right))
    .map((kind) => {
      const kindOptions = options.filter((option) => option.kind === kind)
      const eligibleTargets = buildCapabilityRouteTargets(kindOptions, settings, rows)
      const unavailableTargets = kindOptions
        .filter((option) => !option.runtimeSupported && option.unavailableReason)
        .map((option) => {
          const config = settings.configs.find((candidate) => candidate.id === option.configId)
          return {
            configId: option.configId,
            providerId: option.providerId,
            modelId: config?.modelId ?? '',
            displayName: config?.displayName ?? option.configId,
            operationId: option.operationId,
            reason: option.unavailableReason ?? 'Unavailable',
          }
        })
      const savedRoute =
        routes.find(
          (route) =>
            route.kind === kind &&
            route.enabled &&
            settings.configs.some((config) => config.id === route.configId),
        ) ?? null

      return {
        kind,
        savedRoute,
        selectedTarget: savedRoute
          ? (eligibleTargets.find((target) => target.configId === savedRoute.configId) ??
            buildCapabilityRouteTarget(
              kindOptions.filter((option) => option.configId === savedRoute.configId),
              settings,
              rows,
              savedRoute.operationIds,
            ))
          : null,
        eligibleTargets,
        unavailableTargets,
      }
    })
}

function buildCapabilityRouteTargets(
  options: ProviderCapabilityRouteOption[],
  settings: ListProviderSettingsResponse,
  rows: ModelAssetRow[],
): CapabilityRouteTarget[] {
  const optionsByConfigId = new Map<string, ProviderCapabilityRouteOption[]>()
  for (const option of options) {
    if (!option.runtimeSupported) {
      continue
    }

    optionsByConfigId.set(option.configId, [
      ...(optionsByConfigId.get(option.configId) ?? []),
      option,
    ])
  }

  return [...optionsByConfigId.entries()]
    .map(([, configOptions]) => buildCapabilityRouteTarget(configOptions, settings, rows))
    .filter((target): target is CapabilityRouteTarget => target !== null)
    .sort((left, right) => left.displayName.localeCompare(right.displayName))
}

function buildCapabilityRouteTarget(
  options: ProviderCapabilityRouteOption[],
  settings: ListProviderSettingsResponse,
  rows: ModelAssetRow[],
  operationIds = options.map((option) => option.operationId),
): CapabilityRouteTarget | null {
  const firstOption = options[0]
  const config = firstOption
    ? settings.configs.find((candidate) => candidate.id === firstOption.configId)
    : null
  const row = firstOption
    ? rows.find((candidate) => candidate.configId === firstOption.configId)
    : null
  if (!firstOption || !config || !row) {
    return null
  }

  return {
    configId: firstOption.configId,
    providerId: firstOption.providerId,
    modelId: config.modelId,
    displayName: config.displayName,
    providerDisplayName: row.providerDisplayName,
    operationIds,
    execution: pickExecution(options),
    costRisk: pickHighestCostRisk(options),
    health: row.connectivity,
  }
}

function pickExecution(
  options: ProviderCapabilityRouteOption[],
): ProviderCapabilityRouteOption['execution'] {
  if (options.some((option) => option.execution === 'async_job')) {
    return 'async_job'
  }
  if (options.some((option) => option.execution === 'websocket')) {
    return 'websocket'
  }
  return 'sync'
}

function pickHighestCostRisk(
  options: ProviderCapabilityRouteOption[],
): ProviderCapabilityRouteOption['costRisk'] {
  if (options.some((option) => option.costRisk === 'high')) {
    return 'high'
  }
  if (options.some((option) => option.costRisk === 'medium')) {
    return 'medium'
  }
  return 'low'
}

function capabilityRouteKindSortValue(kind: CapabilityRouteKind): number {
  const index = capabilityRouteKindOrder.indexOf(kind)
  return index === -1 ? capabilityRouteKindOrder.length : index
}

export function isFailingConnectivity(connectivity: ConnectivityDisplayState): boolean {
  if (
    connectivity.status === 'loading' ||
    connectivity.status === 'never_checked' ||
    connectivity.status === 'unavailable'
  ) {
    return false
  }

  return FAILING_PROBE_STATUSES.has(connectivity.status)
}

function isQuerySliceLoading(status: QuerySlice<unknown>['status']): boolean {
  return status === 'loading' || status === 'idle'
}

export function isModelScopedQuota(scope: OfficialQuotaScope): boolean {
  return scope === 'model'
}

function attachRouteBindingsToRows(
  rows: ModelAssetRow[],
  capabilityRoutes: SectionState<CapabilityRouteRow[]>,
): ModelAssetRow[] {
  if (capabilityRoutes.status !== 'ready') {
    return rows.map((row) => ({
      ...row,
      routeBindings: routeBindingsSectionFromCapabilityRoutes(capabilityRoutes),
    }))
  }

  return rows.map((row) => ({
    ...row,
    routeBindings: {
      status: 'ready',
      data: capabilityRoutes.data
        .filter((route) => route.savedRoute?.configId === row.configId)
        .map((route) => ({
          kind: route.kind,
          operationIds: route.savedRoute?.operationIds ?? [],
        })),
    },
  }))
}

function routeBindingsSectionFromCapabilityRoutes(
  capabilityRoutes: Exclude<SectionState<CapabilityRouteRow[]>, { status: 'ready' }>,
): SectionState<ModelRouteBinding[]> {
  if (capabilityRoutes.status === 'error') {
    return { status: 'error', safeMessage: capabilityRoutes.safeMessage }
  }

  return { status: capabilityRoutes.status }
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
