import { Activity, KeyRound, Link2, Server, ShieldCheck, Star } from 'lucide-react'
import type { FormEvent, ReactNode } from 'react'
import { useEffect, useMemo, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { formatMilliseconds, formatNumber, formatTokens } from '@/shared/formatters'
import {
  getProviderConfigApiKey,
  type ModelProviderCatalogResponse,
  type ProviderSettingsRequest,
  requestProviderConfigApiKeyReveal,
  saveProviderSettings,
} from '@/shared/tauri/commands'
import { getCommandErrorMessage } from '@/shared/tauri/errors'
import { useCommandClient } from '@/shared/tauri/react'
import { Badge } from '@/shared/ui/badge'
import { Button } from '@/shared/ui/button'
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from '@/shared/ui/dialog'
import { Input } from '@/shared/ui/input'
import { Select } from '@/shared/ui/select'
import { Tabs, TabsContent, TabsList, TabsTrigger } from '@/shared/ui/tabs'

import { routeKindLabel } from './CapabilityRoutesPanel'
import type {
  CapabilityRouteRow,
  ModelAssetRow,
  QuotaDisplayState,
  UsageDisplayState,
} from './model-settings-view-model'

type ModelDetailsDrawerProps = {
  catalog: ModelProviderCatalogResponse
  open: boolean
  row: ModelAssetRow | null
  onOpenChange: (open: boolean) => void
  onSaved?: () => void | Promise<void>
  onUseForRoute?: (kind: CapabilityRouteRow['kind'], configId: string) => void
}

export function ModelDetailsDrawer({
  catalog,
  onOpenChange,
  onSaved,
  onUseForRoute,
  open,
  row,
}: ModelDetailsDrawerProps) {
  const { t } = useTranslation('settings')
  const commandClient = useCommandClient()
  const secretFormRef = useRef<HTMLFormElement>(null)
  const revealedApiKeyRef = useRef<HTMLElement>(null)
  const [activeTab, setActiveTab] = useState('overview')
  const [displayName, setDisplayName] = useState('')
  const [providerId, setProviderId] = useState('')
  const [modelId, setModelId] = useState('')
  const [baseUrl, setBaseUrl] = useState('')
  const [saveError, setSaveError] = useState<string | null>(null)
  const [isSaving, setIsSaving] = useState(false)
  const [revealedApiKeyVisible, setRevealedApiKeyVisible] = useState(false)
  const [revealError, setRevealError] = useState<string | null>(null)
  const [isRevealing, setIsRevealing] = useState(false)
  const currentConfigId = open && row ? row.configId : null
  const activeConfigIdRef = useRef<string | null>(currentConfigId)
  const lastActiveConfigIdRef = useRef<string | null>(currentConfigId)
  const revealGenerationRef = useRef(0)

  const selectedProvider = useMemo(
    () =>
      catalog.providers.find((provider) => provider.providerId === providerId) ??
      catalog.providers[0],
    [catalog.providers, providerId],
  )
  const modelOptions = useMemo(() => {
    const models = [...(selectedProvider?.models ?? [])]
    if (
      row &&
      row.providerId === selectedProvider?.providerId &&
      row.modelDescriptor &&
      !models.some((model) => model.modelId === row.modelId)
    ) {
      models.push({
        ...row.modelDescriptor,
        displayName: row.modelId,
        modelId: row.modelId,
      })
    }
    return models
  }, [row?.modelDescriptor, row?.modelId, row?.providerId, selectedProvider])

  if (lastActiveConfigIdRef.current !== currentConfigId) {
    lastActiveConfigIdRef.current = currentConfigId
    activeConfigIdRef.current = currentConfigId
    revealGenerationRef.current += 1
  } else {
    activeConfigIdRef.current = currentConfigId
  }

  useEffect(() => {
    setActiveTab('overview')
    setDisplayName(row?.displayName ?? '')
    setProviderId(row?.providerId ?? catalog.providers[0]?.providerId ?? '')
    setModelId(row?.modelId ?? catalog.providers[0]?.models[0]?.modelId ?? '')
    setBaseUrl(row?.baseUrl ?? '')
    setSaveError(null)
    setIsSaving(false)
    clearSecretFormFields(secretFormRef.current)
    clearRevealedApiKey(revealedApiKeyRef.current)
    setRevealedApiKeyVisible(false)
    setRevealError(null)
    setIsRevealing(false)
  }, [currentConfigId])

  useEffect(() => {
    const modelExists = modelOptions.some((model) => model.modelId === modelId)
    const firstModel = modelOptions[0]
    if (!modelExists && firstModel) {
      setModelId(firstModel.modelId)
    }
  }, [modelId, modelOptions])

  function changeOpen(nextOpen: boolean) {
    if (!nextOpen) {
      clearSecretFormFields(secretFormRef.current)
      clearRevealedApiKey(revealedApiKeyRef.current)
      setRevealedApiKeyVisible(false)
    }
    onOpenChange(nextOpen)
  }

  async function revealApiKey() {
    if (!row?.hasApiKey || isRevealing) {
      return
    }

    const configId = row.configId
    const revealGeneration = revealGenerationRef.current + 1
    revealGenerationRef.current = revealGeneration
    setIsRevealing(true)
    setRevealError(null)
    clearRevealedApiKey(revealedApiKeyRef.current)
    setRevealedApiKeyVisible(false)
    try {
      const reveal = await requestProviderConfigApiKeyReveal(configId, commandClient)
      if (
        activeConfigIdRef.current !== configId ||
        revealGenerationRef.current !== revealGeneration
      ) {
        return
      }
      const payload = await getProviderConfigApiKey(configId, reveal.revealToken, commandClient)
      if (
        activeConfigIdRef.current === configId &&
        payload.configId === configId &&
        revealGenerationRef.current === revealGeneration
      ) {
        if (revealedApiKeyRef.current) {
          revealedApiKeyRef.current.textContent = payload.apiKey
        }
        setRevealedApiKeyVisible(true)
      }
    } catch (error) {
      if (
        activeConfigIdRef.current === configId &&
        revealGenerationRef.current === revealGeneration
      ) {
        setRevealError(getCommandErrorMessage(error))
        clearRevealedApiKey(revealedApiKeyRef.current)
        setRevealedApiKeyVisible(false)
      }
    } finally {
      if (
        activeConfigIdRef.current === configId &&
        revealGenerationRef.current === revealGeneration
      ) {
        setIsRevealing(false)
      }
    }
  }

  async function submitConfiguration(event: FormEvent<HTMLFormElement>) {
    event.preventDefault()
    if (!row || isSaving) {
      return
    }

    const form = event.currentTarget
    const request: ProviderSettingsRequest = {
      configId: row.configId,
      modelId,
      providerId,
      setDefault: row.isDefault,
    }
    const providerUnchanged = providerId === row.providerId
    if ((providerId === 'qwen' || providerId === 'minimax') && providerUnchanged) {
      request.protocol = row.protocol
    }
    if (providerUnchanged && row.providerDefaults) {
      request.providerDefaults = row.providerDefaults
    }
    const trimmedDisplayName = displayName.trim()
    const trimmedBaseUrl = baseUrl.trim()
    const apiKey = readSecretFormValue(form, 'apiKey')
    const officialQuotaApiKey = readSecretFormValue(form, 'officialQuotaApiKey')

    if (trimmedDisplayName) {
      request.displayName = trimmedDisplayName
    }
    request.modelOptions =
      providerUnchanged && modelId === row.modelId ? (row.modelOptions ?? {}) : {}
    if (trimmedBaseUrl) {
      request.baseUrl = trimmedBaseUrl
    }
    if (apiKey) {
      request.apiKey = apiKey
    }
    if (officialQuotaApiKey) {
      request.officialQuotaApiKey = officialQuotaApiKey
    }
    if (!row.hasApiKey && !apiKey) {
      setSaveError(t('provider.errors.apiKeyRequired'))
      return
    }

    setIsSaving(true)
    setSaveError(null)
    try {
      await saveProviderSettings(request, commandClient)
      clearSecretFormFields(form)
      clearRevealedApiKey(revealedApiKeyRef.current)
      setRevealedApiKeyVisible(false)
      await onSaved?.()
    } catch (error) {
      clearSecretFormFields(form)
      setSaveError(getCommandErrorMessage(error))
    } finally {
      setIsSaving(false)
    }
  }

  return (
    <Dialog onOpenChange={changeOpen} open={open && row !== null}>
      {row ? (
        <DialogContent className="max-h-[calc(100vh-2rem)] w-[min(880px,94vw)] overflow-y-auto">
          <DialogHeader>
            <DialogTitle>{row.displayName}</DialogTitle>
            <DialogDescription>
              {row.providerDisplayName} / {row.modelId}
            </DialogDescription>
          </DialogHeader>

          <Tabs onValueChange={setActiveTab} value={activeTab}>
            <TabsList className="h-auto flex-wrap justify-start">
              <TabsTrigger onClick={() => setActiveTab('overview')} value="overview">
                {t('models.details.tabs.overview')}
              </TabsTrigger>
              <TabsTrigger onClick={() => setActiveTab('quota')} value="quota">
                {t('models.details.tabs.quota')}
              </TabsTrigger>
              <TabsTrigger onClick={() => setActiveTab('capabilities')} value="capabilities">
                {t('models.details.tabs.capabilities')}
              </TabsTrigger>
            </TabsList>

            <TabsContent className="space-y-4" value="overview">
              <OverviewStatusStrip row={row} />

              <section className="space-y-3">
                <SectionTitle>{t('models.details.overview.connectionAndUsage')}</SectionTitle>
                <DetailGrid>
                  <DetailRow label={t('models.columns.latency')}>
                    {formatConnectivityMilliseconds(
                      row.connectivity,
                      'latencyMs',
                      t('models.summary.loadingMetric'),
                      t('models.unavailable'),
                    )}
                  </DetailRow>
                  <DetailRow label={t('models.columns.timeout')}>
                    {formatConnectivityMilliseconds(
                      row.connectivity,
                      'timeoutMs',
                      t('models.summary.loadingMetric'),
                      t('models.unavailable'),
                    )}
                  </DetailRow>
                  <DetailRow label={t('models.details.connectivity.checkedAt')}>
                    {row.connectivity.status === 'loading'
                      ? t('models.summary.loadingMetric')
                      : 'checkedAt' in row.connectivity
                        ? row.connectivity.checkedAt
                        : t('models.unavailable')}
                  </DetailRow>
                  {'safeMessage' in row.connectivity && row.connectivity.safeMessage ? (
                    <DetailRow label={t('models.details.safeMessage')}>
                      {row.connectivity.safeMessage}
                    </DetailRow>
                  ) : null}
                </DetailGrid>
                <UsageDetails usage={row.usage} />
              </section>

              <section className="space-y-3">
                <SectionTitle>{t('models.details.overview.configuration')}</SectionTitle>
                <form
                  className="grid gap-3 rounded-sm border border-border bg-background p-3"
                  onSubmit={(event) => void submitConfiguration(event)}
                  ref={secretFormRef}
                >
                  <div className="grid gap-3 sm:grid-cols-2">
                    <label className="grid gap-1 text-sm" htmlFor="model-details-display-name">
                      <span className="font-medium">{t('provider.profileName')}</span>
                      <Input
                        id="model-details-display-name"
                        onChange={(event) => setDisplayName(event.target.value)}
                        value={displayName}
                      />
                    </label>

                    <label className="grid gap-1 text-sm" htmlFor="model-details-provider-id">
                      <span className="font-medium">{t('provider.provider')}</span>
                      <Select
                        id="model-details-provider-id"
                        onChange={(event) => setProviderId(event.target.value)}
                        value={providerId}
                      >
                        {catalog.providers.map((provider) => (
                          <option key={provider.providerId} value={provider.providerId}>
                            {provider.displayName}
                          </option>
                        ))}
                      </Select>
                    </label>

                    <label className="grid gap-1 text-sm" htmlFor="model-details-model-id">
                      <span className="font-medium">{t('provider.model')}</span>
                      <Select
                        id="model-details-model-id"
                        onChange={(event) => setModelId(event.target.value)}
                        value={modelId}
                      >
                        {modelOptions.map((model) => (
                          <option key={model.modelId} value={model.modelId}>
                            {model.displayName}
                          </option>
                        ))}
                      </Select>
                    </label>

                    <label className="grid gap-1 text-sm" htmlFor="model-details-base-url">
                      <span className="font-medium">{t('provider.baseUrl')}</span>
                      <Input
                        id="model-details-base-url"
                        onChange={(event) => setBaseUrl(event.target.value)}
                        placeholder={selectedProvider?.defaultBaseUrl}
                        value={baseUrl}
                      />
                    </label>

                    <label className="grid gap-1 text-sm" htmlFor="model-details-api-key">
                      <span className="flex items-center justify-between gap-2 font-medium">
                        {t('provider.apiKey')}
                        <SavedStateBadge saved={row.hasApiKey}>
                          {row.hasApiKey
                            ? t('models.details.configuration.apiKeySaved')
                            : t('models.details.configuration.apiKeyMissing')}
                        </SavedStateBadge>
                      </span>
                      <Input
                        aria-label={t('provider.apiKey')}
                        id="model-details-api-key"
                        name="apiKey"
                        placeholder={
                          row.hasApiKey
                            ? t('provider.apiKeyExistingPlaceholder')
                            : t('provider.apiKeyPlaceholder')
                        }
                        type="password"
                      />
                    </label>

                    <label
                      className="grid gap-1 text-sm"
                      htmlFor="model-details-official-quota-api-key"
                    >
                      <span className="flex items-center justify-between gap-2 font-medium">
                        {t('provider.officialQuotaApiKey')}
                        <SavedStateBadge saved={row.hasOfficialQuotaApiKey}>
                          {row.hasOfficialQuotaApiKey
                            ? t('provider.savedApiKeyAvailable')
                            : t('provider.savedApiKeyMissing')}
                        </SavedStateBadge>
                      </span>
                      <Input
                        aria-label={t('provider.officialQuotaApiKey')}
                        id="model-details-official-quota-api-key"
                        name="officialQuotaApiKey"
                        placeholder={
                          row.hasOfficialQuotaApiKey
                            ? t('provider.officialQuotaApiKeyExistingPlaceholder')
                            : t('provider.officialQuotaApiKeyPlaceholder')
                        }
                        type="password"
                      />
                    </label>
                  </div>

                  {row.hasApiKey ? (
                    <div className="space-y-2">
                      <Button
                        disabled={isRevealing}
                        onClick={() => void revealApiKey()}
                        type="button"
                        variant="outline"
                      >
                        {isRevealing ? t('provider.revealingApiKey') : t('provider.revealApiKey')}
                      </Button>
                      <div
                        className="grid gap-1 rounded-sm border border-border bg-muted px-3 py-2 text-sm"
                        hidden={!revealedApiKeyVisible}
                      >
                        <span className="text-muted-foreground text-xs">
                          {t('provider.savedApiKey')}
                        </span>
                        <code
                          className="break-all font-mono text-foreground"
                          ref={revealedApiKeyRef}
                        />
                      </div>
                      {revealError ? (
                        <p className="text-destructive text-sm" role="alert">
                          {revealError}
                        </p>
                      ) : null}
                    </div>
                  ) : null}

                  {saveError ? (
                    <p className="text-destructive text-sm" role="alert">
                      {saveError}
                    </p>
                  ) : null}

                  <div className="flex justify-end">
                    <Button disabled={isSaving} type="submit">
                      {isSaving ? t('provider.saving') : t('provider.save')}
                    </Button>
                  </div>
                </form>
              </section>
            </TabsContent>

            <TabsContent value="quota">
              <QuotaDetails quota={row.quota} />
            </TabsContent>

            <TabsContent value="capabilities">
              <CapabilitiesDetails onUseForRoute={onUseForRoute} row={row} />
            </TabsContent>
          </Tabs>
        </DialogContent>
      ) : null}
    </Dialog>
  )
}

function OverviewStatusStrip({ row }: { row: ModelAssetRow }) {
  const { t } = useTranslation('settings')
  const connectivityTone = connectivityToneClass(row.connectivity.status)

  return (
    <section className="grid gap-2 sm:grid-cols-2 lg:grid-cols-4">
      <StatusCard
        icon={<Server aria-hidden="true" className="size-4" data-icon />}
        label={t('models.columns.provider')}
        value={row.providerDisplayName}
      />
      <StatusCard
        icon={<Link2 aria-hidden="true" className="size-4" data-icon />}
        label={t('models.columns.identity')}
        value={row.modelId}
      />
      <StatusCard
        icon={<Star aria-hidden="true" className="size-4" data-icon />}
        label={t('models.columns.default')}
        tone={row.isDefault ? 'text-primary bg-primary/10 border-primary/20' : undefined}
        value={row.isDefault ? t('models.defaultMarker') : t('models.details.notDefault')}
      />
      <StatusCard
        icon={<Activity aria-hidden="true" className="size-4" data-icon />}
        label={t('models.columns.connectivity')}
        tone={connectivityTone}
        value={<ConnectivityText row={row} />}
      />
    </section>
  )
}

function StatusCard({
  icon,
  label,
  tone,
  value,
}: {
  icon: ReactNode
  label: string
  tone?: string
  value: ReactNode
}) {
  return (
    <div className="grid min-w-0 grid-cols-[auto_minmax(0,1fr)] gap-2 rounded-sm border border-border bg-muted/40 px-3 py-2">
      <div
        className={[
          'mt-0.5 flex size-7 items-center justify-center rounded-sm border border-border bg-background text-muted-foreground',
          tone ?? '',
        ].join(' ')}
      >
        {icon}
      </div>
      <div className="min-w-0">
        <div className="text-muted-foreground text-xs">{label}</div>
        <div className="truncate font-medium text-sm">{value}</div>
      </div>
    </div>
  )
}

function SavedStateBadge({ children, saved }: { children: ReactNode; saved: boolean }) {
  return (
    <span
      className={[
        'inline-flex shrink-0 items-center gap-1 rounded-sm border px-1.5 py-0.5 font-medium text-[11px]',
        saved
          ? 'border-success/25 bg-success/10 text-success'
          : 'border-border bg-muted text-muted-foreground',
      ].join(' ')}
    >
      {saved ? (
        <ShieldCheck aria-hidden="true" className="size-3" data-icon />
      ) : (
        <KeyRound aria-hidden="true" className="size-3" data-icon />
      )}
      {children}
    </span>
  )
}

function CapabilitiesDetails({
  onUseForRoute,
  row,
}: {
  row: ModelAssetRow
  onUseForRoute?: ModelDetailsDrawerProps['onUseForRoute']
}) {
  const { t } = useTranslation('settings')
  const capability = row.modelDescriptor?.conversationCapability

  if (!capability) {
    return <p className="text-muted-foreground text-sm">{t('models.unavailable')}</p>
  }

  return (
    <div className="space-y-4">
      <DetailGrid>
        <DetailRow label={t('provider.capability.streaming')}>
          {formatBoolean(capability.streaming, t('yes'), t('no'))}
        </DetailRow>
        <DetailRow label={t('provider.capability.tools')}>
          {formatBoolean(capability.toolCalling, t('yes'), t('no'))}
        </DetailRow>
        <DetailRow label={t('provider.capability.structuredOutput')}>
          {formatBoolean(capability.structuredOutput, t('yes'), t('no'))}
        </DetailRow>
        <DetailRow label={t('models.details.capabilities.contextWindow')}>
          {formatNumber(capability.contextWindow)}
        </DetailRow>
        <DetailRow label={t('models.details.capabilities.maxOutputTokens')}>
          {formatNumber(capability.maxOutputTokens)}
        </DetailRow>
      </DetailGrid>

      <section className="space-y-2">
        <h3 className="font-medium text-sm">{t('models.details.capabilities.routeBindings')}</h3>
        {row.routeBindings.status === 'loading' ? (
          <p className="text-muted-foreground text-sm">{t('models.summary.loadingMetric')}</p>
        ) : row.routeBindings.status === 'error' ? (
          <p className="text-destructive text-sm" role="alert">
            {row.routeBindings.safeMessage}
          </p>
        ) : row.routeBindings.status === 'unavailable' ? (
          <p className="text-muted-foreground text-sm">{t('models.unavailable')}</p>
        ) : row.routeBindings.data.length > 0 ? (
          <div className="space-y-2">
            {row.routeBindings.data.map((binding) => (
              <div className="rounded-md border border-border bg-background p-3" key={binding.kind}>
                <div className="flex flex-wrap items-center justify-between gap-2">
                  <div>
                    <div className="font-medium text-sm">{routeKindLabel(binding.kind, t)}</div>
                    <div className="mt-1 text-muted-foreground text-xs">
                      {binding.operationIds.join(', ')}
                    </div>
                  </div>
                  {onUseForRoute ? (
                    <Button
                      onClick={() => onUseForRoute(binding.kind, row.configId)}
                      type="button"
                      variant="outline"
                    >
                      {t('models.details.capabilities.useForRoute', {
                        kind: routeKindLabel(binding.kind, t).toLowerCase(),
                      })}
                    </Button>
                  ) : null}
                </div>
              </div>
            ))}
          </div>
        ) : (
          <p className="text-muted-foreground text-sm">
            {t('models.details.capabilities.noRouteBindings')}
          </p>
        )}
      </section>
    </div>
  )
}

function UsageDetails({ usage }: { usage: UsageDisplayState }) {
  const { t } = useTranslation('settings')

  if (usage.status === 'loading') {
    return <p className="text-muted-foreground text-sm">{t('models.summary.loadingMetric')}</p>
  }

  if (usage.status === 'unavailable') {
    return <p className="text-muted-foreground text-sm">{t('models.unavailable')}</p>
  }

  return (
    <div className="space-y-3">
      <Badge variant="outline">{t('models.details.usage.modelLevel')}</Badge>
      {usage.sharedModelUsage ? (
        <p className="rounded-sm border border-border bg-muted px-3 py-2 text-muted-foreground text-sm">
          {t('models.details.usage.sharedModelUsage')}
        </p>
      ) : null}
      <DetailGrid>
        <DetailRow label={t('models.columns.todayUsage')}>{formatUsage(usage.today)}</DetailRow>
        <DetailRow label={t('models.columns.monthUsage')}>
          {formatUsage(usage.monthToDate)}
        </DetailRow>
        <DetailRow label={t('models.columns.totalUsage')}>{formatUsage(usage.allTime)}</DetailRow>
      </DetailGrid>
    </div>
  )
}

function QuotaDetails({ quota }: { quota: QuotaDisplayState }) {
  const { t } = useTranslation('settings')

  if (quota.status === 'loading') {
    return <p className="text-muted-foreground text-sm">{t('models.summary.loadingMetric')}</p>
  }

  if (quota.status === 'unavailable') {
    return <p className="text-muted-foreground text-sm">{t('models.unavailable')}</p>
  }

  return (
    <DetailGrid>
      <DetailRow label={t('models.details.quota.scope')}>
        {t(`models.quotaScope.${quota.scopeLabel}`)}
      </DetailRow>
      <DetailRow label={t('models.columns.quota')}>{t(`models.quota.${quota.status}`)}</DetailRow>
      <DetailRow label={t('models.details.quota.source')}>{quota.sourceUrl}</DetailRow>
      <DetailRow label={t('models.details.quota.fetchedAt')}>{quota.fetchedAt}</DetailRow>
      <DetailRow label={t('models.details.quota.expiresAt')}>{quota.expiresAt}</DetailRow>
      {quota.quotaUsed !== undefined || quota.quotaTotal !== undefined ? (
        <DetailRow label={t('models.details.quota.amount')}>
          {quota.quotaUsed ?? '-'} / {quota.quotaTotal ?? '-'} {quota.unit ?? ''}
        </DetailRow>
      ) : null}
      {quota.safeMessage ? (
        <DetailRow label={t('models.details.safeMessage')}>{quota.safeMessage}</DetailRow>
      ) : null}
    </DetailGrid>
  )
}

function ConnectivityText({ row }: { row: ModelAssetRow }) {
  const { t } = useTranslation('settings')
  const status = row.connectivity.status
  if (status === 'loading') {
    return t('models.summary.loadingMetric')
  }
  return status === 'unavailable' ? t('models.unavailable') : t(`models.connectivity.${status}`)
}

function SectionTitle({ children }: { children: ReactNode }) {
  return <h3 className="font-medium text-sm">{children}</h3>
}

function DetailGrid({ children }: { children: ReactNode }) {
  return <dl className="grid gap-3 text-sm sm:grid-cols-2">{children}</dl>
}

function DetailRow({ children, label }: { children: ReactNode; label: string }) {
  return (
    <div className="grid gap-1 rounded-sm border border-border bg-background px-3 py-2">
      <dt className="text-muted-foreground text-xs">{label}</dt>
      <dd className="min-w-0 break-words font-medium">{children}</dd>
    </div>
  )
}

function connectivityToneClass(
  status: ModelAssetRow['connectivity']['status'],
): string | undefined {
  if (status === 'online') {
    return 'border-success/25 bg-success/10 text-success'
  }
  if (status === 'loading' || status === 'never_checked') {
    return 'border-warning/25 bg-warning/10 text-warning'
  }
  if (status === 'unavailable' || status === 'unsupported') {
    return 'border-border bg-muted text-muted-foreground'
  }
  return 'border-destructive/25 bg-destructive/10 text-destructive'
}

function formatConnectivityMilliseconds(
  connectivity: ModelAssetRow['connectivity'],
  key: 'latencyMs' | 'timeoutMs',
  loading: string,
  unavailable: string,
) {
  if (connectivity.status === 'loading') {
    return loading
  }
  if (connectivity.status === 'never_checked' || connectivity.status === 'unavailable') {
    return unavailable
  }
  return formatMilliseconds(connectivity[key], unavailable)
}

function formatBoolean(value: boolean, yes: string, no: string) {
  return value ? yes : no
}

function formatUsage(usage: {
  cacheReadTokens: number
  cacheWriteTokens: number
  inputTokens: number
  outputTokens: number
}) {
  const tokens =
    usage.inputTokens + usage.outputTokens + usage.cacheReadTokens + usage.cacheWriteTokens
  return formatTokens(tokens)
}

function readSecretFormValue(form: HTMLFormElement, name: string): string {
  const value = new FormData(form).get(name)
  return typeof value === 'string' ? value.trim() : ''
}

function clearSecretFormFields(form: HTMLFormElement | null) {
  if (!form) {
    return
  }
  for (const name of ['apiKey', 'officialQuotaApiKey']) {
    const field = form.elements.namedItem(name)
    if (field instanceof HTMLInputElement) {
      field.value = ''
    }
  }
}

function clearRevealedApiKey(element: HTMLElement | null) {
  if (element) {
    element.textContent = ''
  }
}
