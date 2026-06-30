import { useEffect, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'

import { getProviderConfigApiKey, requestProviderConfigApiKeyReveal } from '@/shared/tauri/commands'
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
import { Tabs, TabsContent, TabsList, TabsTrigger } from '@/shared/ui/tabs'

import { routeKindLabel } from './CapabilityRoutesPanel'
import type {
  ModelAssetRow,
  QuotaDisplayState,
  UsageDisplayState,
} from './model-settings-view-model'

type ModelDetailsDrawerProps = {
  open: boolean
  row: ModelAssetRow | null
  onOpenChange: (open: boolean) => void
  onEdit?: (row: ModelAssetRow) => void
  onUseForRoute?: (kind: ModelAssetRow['routeBindings'][number]['kind'], configId: string) => void
}

type RevealedApiKeyState = {
  apiKey: string
  configId: string
  generation: number
}

export function ModelDetailsDrawer({
  onEdit,
  onOpenChange,
  onUseForRoute,
  open,
  row,
}: ModelDetailsDrawerProps) {
  const { t } = useTranslation('settings')
  const commandClient = useCommandClient()
  const [activeTab, setActiveTab] = useState('overview')
  const [revealedApiKey, setRevealedApiKey] = useState<RevealedApiKeyState | null>(null)
  const [revealError, setRevealError] = useState<string | null>(null)
  const [isRevealing, setIsRevealing] = useState(false)
  const currentConfigId = open && row ? row.configId : null
  const activeConfigIdRef = useRef<string | null>(currentConfigId)
  const lastActiveConfigIdRef = useRef<string | null>(currentConfigId)
  const revealGenerationRef = useRef(0)

  if (lastActiveConfigIdRef.current !== currentConfigId) {
    lastActiveConfigIdRef.current = currentConfigId
    activeConfigIdRef.current = currentConfigId
    revealGenerationRef.current += 1
  } else {
    activeConfigIdRef.current = currentConfigId
  }

  useEffect(() => {
    setActiveTab('overview')
    setRevealedApiKey(null)
    setRevealError(null)
    setIsRevealing(false)
  }, [currentConfigId])

  async function revealApiKey() {
    if (!row?.hasApiKey || isRevealing) {
      return
    }

    const configId = row.configId
    const revealGeneration = revealGenerationRef.current + 1
    revealGenerationRef.current = revealGeneration
    setIsRevealing(true)
    setRevealError(null)
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
        revealGenerationRef.current === revealGeneration
      ) {
        setRevealedApiKey({
          apiKey: payload.apiKey,
          configId,
          generation: revealGeneration,
        })
      }
    } catch (error) {
      if (
        activeConfigIdRef.current === configId &&
        revealGenerationRef.current === revealGeneration
      ) {
        setRevealError(getCommandErrorMessage(error))
        setRevealedApiKey(null)
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

  const visibleApiKey =
    revealedApiKey &&
    revealedApiKey.configId === currentConfigId &&
    revealedApiKey.generation === revealGenerationRef.current
      ? revealedApiKey.apiKey
      : null

  return (
    <Dialog onOpenChange={onOpenChange} open={open && row !== null}>
      {row ? (
        <DialogContent className="right-4 left-auto top-4 max-h-[calc(100vh-2rem)] w-[min(720px,92vw)] translate-x-0 translate-y-0 overflow-y-auto sm:rounded-md">
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
              <TabsTrigger onClick={() => setActiveTab('connectivity')} value="connectivity">
                {t('models.details.tabs.connectivity')}
              </TabsTrigger>
              <TabsTrigger onClick={() => setActiveTab('usage')} value="usage">
                {t('models.details.tabs.usage')}
              </TabsTrigger>
              <TabsTrigger onClick={() => setActiveTab('quota')} value="quota">
                {t('models.details.tabs.quota')}
              </TabsTrigger>
              <TabsTrigger onClick={() => setActiveTab('configuration')} value="configuration">
                {t('models.details.tabs.configuration')}
              </TabsTrigger>
              <TabsTrigger onClick={() => setActiveTab('capabilities')} value="capabilities">
                {t('models.details.tabs.capabilities')}
              </TabsTrigger>
            </TabsList>

            <TabsContent value="overview">
              <DetailGrid>
                <DetailRow label={t('models.columns.provider')}>
                  {row.providerDisplayName}
                </DetailRow>
                <DetailRow label={t('models.columns.identity')}>{row.modelId}</DetailRow>
                <DetailRow label={t('models.columns.default')}>
                  {row.isDefault ? t('models.defaultMarker') : t('models.details.notDefault')}
                </DetailRow>
                <DetailRow label={t('models.columns.connectivity')}>
                  <ConnectivityText row={row} />
                </DetailRow>
              </DetailGrid>
            </TabsContent>

            <TabsContent value="connectivity">
              <DetailGrid>
                <DetailRow label={t('models.details.connectivity.status')}>
                  <ConnectivityText row={row} />
                </DetailRow>
                <DetailRow label={t('models.columns.timeout')}>
                  {formatMilliseconds(
                    'timeoutMs' in row.connectivity ? row.connectivity.timeoutMs : undefined,
                    t('models.unavailable'),
                  )}
                </DetailRow>
                <DetailRow label={t('models.columns.latency')}>
                  {formatMilliseconds(
                    'latencyMs' in row.connectivity ? row.connectivity.latencyMs : undefined,
                    t('models.unavailable'),
                  )}
                </DetailRow>
                <DetailRow label={t('models.details.connectivity.checkedAt')}>
                  {'checkedAt' in row.connectivity
                    ? row.connectivity.checkedAt
                    : t('models.unavailable')}
                </DetailRow>
                {'safeMessage' in row.connectivity && row.connectivity.safeMessage ? (
                  <DetailRow label={t('models.details.safeMessage')}>
                    {row.connectivity.safeMessage}
                  </DetailRow>
                ) : null}
              </DetailGrid>
            </TabsContent>

            <TabsContent value="usage">
              <UsageDetails usage={row.usage} />
            </TabsContent>

            <TabsContent value="quota">
              <QuotaDetails quota={row.quota} />
            </TabsContent>

            <TabsContent value="configuration">
              <DetailGrid>
                <DetailRow label={t('provider.profileName')}>{row.displayName}</DetailRow>
                <DetailRow label={t('provider.provider')}>{row.providerDisplayName}</DetailRow>
                <DetailRow label={t('provider.model')}>{row.modelId}</DetailRow>
                <DetailRow label={t('provider.apiKey')}>
                  {row.hasApiKey
                    ? t('models.details.configuration.apiKeySaved')
                    : t('models.details.configuration.apiKeyMissing')}
                </DetailRow>
              </DetailGrid>
              {row.hasApiKey ? (
                <div className="mt-4 space-y-2">
                  <Button
                    disabled={isRevealing}
                    onClick={() => void revealApiKey()}
                    type="button"
                    variant="outline"
                  >
                    {isRevealing ? t('provider.revealingApiKey') : t('provider.revealApiKey')}
                  </Button>
                  {visibleApiKey ? (
                    <code className="block rounded-sm border border-border bg-muted px-2 py-1 text-sm">
                      {visibleApiKey}
                    </code>
                  ) : null}
                  {revealError ? (
                    <p className="text-destructive text-sm" role="alert">
                      {revealError}
                    </p>
                  ) : null}
                </div>
              ) : null}
              {onEdit ? (
                <Button className="mt-4" onClick={() => onEdit(row)} type="button">
                  {t('models.details.configuration.edit')}
                </Button>
              ) : null}
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
          {capability.contextWindow.toLocaleString()}
        </DetailRow>
        <DetailRow label={t('models.details.capabilities.maxOutputTokens')}>
          {capability.maxOutputTokens.toLocaleString()}
        </DetailRow>
      </DetailGrid>

      <section className="space-y-2">
        <h3 className="font-medium text-sm">{t('models.details.capabilities.routeBindings')}</h3>
        {row.routeBindings.length > 0 ? (
          <div className="space-y-2">
            {row.routeBindings.map((binding) => (
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
  return status === 'unavailable' ? t('models.unavailable') : t(`models.connectivity.${status}`)
}

function DetailGrid({ children }: { children: React.ReactNode }) {
  return <dl className="grid gap-3 text-sm">{children}</dl>
}

function DetailRow({ children, label }: { children: React.ReactNode; label: string }) {
  return (
    <div className="grid gap-1 rounded-sm border border-border bg-background px-3 py-2">
      <dt className="text-muted-foreground text-xs">{label}</dt>
      <dd className="min-w-0 break-words font-medium">{children}</dd>
    </div>
  )
}

function formatMilliseconds(value: number | undefined, unavailable: string) {
  return value === undefined ? unavailable : `${value.toLocaleString()} ms`
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
  return `${tokens.toLocaleString()} tokens`
}
