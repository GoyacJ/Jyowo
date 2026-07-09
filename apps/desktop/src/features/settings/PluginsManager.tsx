import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { AlertTriangle, FileCode2, Plug, RefreshCw, Settings, Trash2, Upload } from 'lucide-react'
import { type FormEvent, type ReactNode, useEffect, useMemo, useState } from 'react'
import { useTranslation } from 'react-i18next'

import type {
  GetPluginDetailResponse,
  PluginConfigUpdate,
  PluginDetail,
  PluginInstallReport,
  PluginRuntimeCapability,
  PluginSummary,
} from '@/shared/tauri/commands'
import {
  getPluginDetail,
  installPluginFromPath,
  listPlugins,
  reloadPlugin,
  setPluginEnabled,
  setProjectPluginsEnabled,
  uninstallPlugin,
  updatePluginConfig,
  validatePluginFromPath,
} from '@/shared/tauri/commands'
import { pickPluginPackagePath } from '@/shared/tauri/file-dialog'
import { useCommandClient } from '@/shared/tauri/react'
import { Badge } from '@/shared/ui/badge'
import { Button } from '@/shared/ui/button'
import { Checkbox } from '@/shared/ui/checkbox'
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/shared/ui/dialog'
import { IconButton } from '@/shared/ui/icon-button'
import { Input } from '@/shared/ui/input'
import { Section, SectionDescription, SectionHeader, SectionTitle } from '@/shared/ui/section'
import { StatusBadge, type StatusBadgeProps } from '@/shared/ui/status-badge'
import { Switch } from '@/shared/ui/switch'

const pluginQueryKeys = {
  all: ['plugins'] as const,
  detail: (pluginId: string | null) => [...pluginQueryKeys.all, 'detail', pluginId] as const,
  list: () => [...pluginQueryKeys.all, 'list'] as const,
}

const supportedConfigTypes = new Set(['string', 'number', 'boolean', 'path', 'url'])

type ConfigFieldType = 'string' | 'number' | 'boolean' | 'path' | 'url'
type EditableConfigValue = string | number | boolean

type ConfigField = {
  name: string
  secret: boolean
  type: ConfigFieldType
}

export type PluginOpenRequest = {
  pluginId: string
  requestId: number
}

export function PluginsManager({
  openPluginRequest = null,
}: {
  openPluginRequest?: PluginOpenRequest | null
}) {
  const { t } = useTranslation('settings')
  const commandClient = useCommandClient()
  const queryClient = useQueryClient()
  const pluginsQuery = useQuery({
    queryKey: pluginQueryKeys.list(),
    queryFn: () => listPlugins(commandClient),
  })
  const validateMutation = useMutation({
    mutationFn: (sourcePath: string) => validatePluginFromPath(sourcePath, commandClient),
  })
  const installMutation = useMutation({
    mutationFn: (sourcePath: string) => installPluginFromPath(sourcePath, commandClient),
    onSuccess: async () => {
      await queryClient.invalidateQueries({ queryKey: pluginQueryKeys.all })
    },
  })
  const setEnabledMutation = useMutation({
    mutationFn: ({ enabled, pluginId }: { enabled: boolean; pluginId: string }) =>
      setPluginEnabled(pluginId, enabled, commandClient),
    onSuccess: async (response) => {
      await queryClient.invalidateQueries({ queryKey: pluginQueryKeys.list() })
      await queryClient.invalidateQueries({
        queryKey: pluginQueryKeys.detail(response.pluginId ?? null),
      })
    },
  })
  const reloadMutation = useMutation({
    mutationFn: (pluginId: string) => reloadPlugin(pluginId, commandClient),
    onSuccess: async (response) => {
      await queryClient.invalidateQueries({ queryKey: pluginQueryKeys.list() })
      await queryClient.invalidateQueries({
        queryKey: pluginQueryKeys.detail(response.pluginId ?? null),
      })
    },
  })
  const uninstallMutation = useMutation({
    mutationFn: (pluginId: string) => uninstallPlugin(pluginId, commandClient),
    onSuccess: async () => {
      await queryClient.invalidateQueries({ queryKey: pluginQueryKeys.all })
    },
  })
  const setProjectPluginsMutation = useMutation({
    mutationFn: (enabled: boolean) => setProjectPluginsEnabled(enabled, commandClient),
    onSuccess: async () => {
      await queryClient.invalidateQueries({ queryKey: pluginQueryKeys.list() })
    },
  })
  const [installReport, setInstallReport] = useState<PluginInstallReport | null>(null)
  const [installSourcePath, setInstallSourcePath] = useState<string | null>(null)
  const [installDialogOpen, setInstallDialogOpen] = useState(false)
  const [installError, setInstallError] = useState(false)
  const [selectedPluginId, setSelectedPluginId] = useState<string | null>(null)
  const [pendingUninstall, setPendingUninstall] = useState<PluginSummary | null>(null)
  const plugins = pluginsQuery.data?.plugins ?? []
  const allowProjectPlugins = pluginsQuery.data?.allowProjectPlugins ?? false

  useEffect(() => {
    if (openPluginRequest) {
      setSelectedPluginId(openPluginRequest.pluginId)
    }
  }, [openPluginRequest])

  async function pickAndValidatePlugin() {
    const selected = await pickPluginPackagePath()

    if (selected === null) {
      return
    }

    setInstallError(false)
    setInstallReport(null)
    setInstallSourcePath(selected)
    setInstallDialogOpen(true)

    try {
      const report = await validateMutation.mutateAsync(selected)
      setInstallReport(report)
    } catch {
      setInstallError(true)
    }
  }

  async function confirmInstall() {
    if (!installReport?.valid || installSourcePath === null) {
      return
    }

    try {
      await installMutation.mutateAsync(installSourcePath)
      setInstallDialogOpen(false)
      setInstallReport(null)
      setInstallSourcePath(null)
    } catch {
      setInstallError(true)
    }
  }

  async function confirmUninstall() {
    if (pendingUninstall === null) {
      return
    }

    await uninstallMutation.mutateAsync(pendingUninstall.id)
    setPendingUninstall(null)
    if (selectedPluginId === pendingUninstall.id) {
      setSelectedPluginId(null)
    }
  }

  return (
    <Section>
      <div className="flex flex-wrap items-start justify-between gap-3">
        <SectionHeader className="flex items-start gap-3">
          <div className="rounded-md border border-border bg-background p-2 text-muted-foreground">
            <Plug className="size-4" />
          </div>
          <div>
            <SectionTitle>{t('plugins.title')}</SectionTitle>
            <SectionDescription>{t('plugins.description')}</SectionDescription>
          </div>
        </SectionHeader>

        <Button
          disabled={validateMutation.isPending || installMutation.isPending}
          onClick={pickAndValidatePlugin}
          size="sm"
          type="button"
        >
          <Upload data-icon className="size-4" />
          {t('plugins.actions.installLocal')}
        </Button>
      </div>

      {pluginsQuery.isLoading ? (
        <div className="text-muted-foreground text-sm">{t('plugins.loading')}</div>
      ) : null}

      {pluginsQuery.isError ? <ErrorMessage>{t('plugins.loadError')}</ErrorMessage> : null}

      {setEnabledMutation.isError ||
      reloadMutation.isError ||
      uninstallMutation.isError ||
      setProjectPluginsMutation.isError ? (
        <ErrorMessage>{t('plugins.operationError')}</ErrorMessage>
      ) : null}

      <div className="flex flex-wrap items-center justify-between gap-3 rounded-md border border-border bg-background px-3 py-3">
        <div>
          <div className="font-medium text-sm">{t('plugins.projectGate.title')}</div>
          <p className="mt-1 text-muted-foreground text-xs">
            {t('plugins.projectGate.description')}
          </p>
        </div>
        <Switch
          aria-label={t(
            allowProjectPlugins
              ? 'plugins.actions.disableProjectPlugins'
              : 'plugins.actions.enableProjectPlugins',
          )}
          checked={allowProjectPlugins}
          disabled={pluginsQuery.isLoading || setProjectPluginsMutation.isPending}
          onCheckedChange={(enabled) => setProjectPluginsMutation.mutate(enabled)}
        />
      </div>

      {!pluginsQuery.isLoading && !pluginsQuery.isError && plugins.length === 0 ? (
        <div className="rounded-md border border-dashed border-border bg-background px-4 py-6 text-center text-muted-foreground text-sm">
          {t('plugins.empty')}
        </div>
      ) : null}

      {plugins.length > 0 ? (
        <div className="grid gap-3 md:grid-cols-2">
          {plugins.map((plugin) => (
            <PluginCard
              key={plugin.id}
              onOpenDetail={setSelectedPluginId}
              onReload={(pluginId) => reloadMutation.mutate(pluginId)}
              onToggle={(pluginId, enabled) => setEnabledMutation.mutate({ enabled, pluginId })}
              onUninstall={setPendingUninstall}
              operationPending={
                setEnabledMutation.isPending ||
                reloadMutation.isPending ||
                uninstallMutation.isPending
              }
              plugin={plugin}
            />
          ))}
        </div>
      ) : null}

      <InstallCandidateDialog
        error={installError}
        installPending={installMutation.isPending}
        onConfirm={confirmInstall}
        onOpenChange={(open) => {
          setInstallDialogOpen(open)
          if (!open) {
            setInstallReport(null)
            setInstallSourcePath(null)
            setInstallError(false)
          }
        }}
        open={installDialogOpen}
        report={installReport}
        validating={validateMutation.isPending}
      />

      <PluginDetailDialog
        onOpenChange={(open) => {
          if (!open) {
            setSelectedPluginId(null)
          }
        }}
        open={selectedPluginId !== null}
        pluginId={selectedPluginId}
      />

      <Dialog
        onOpenChange={(open) => {
          if (!open) {
            setPendingUninstall(null)
          }
        }}
        open={pendingUninstall !== null}
      >
        <DialogContent>
          <DialogHeader>
            <DialogTitle>{t('plugins.uninstall.title')}</DialogTitle>
            <DialogDescription>
              {t('plugins.uninstall.description', { name: pendingUninstall?.name ?? '' })}
            </DialogDescription>
          </DialogHeader>
          <DialogFooter>
            <Button
              disabled={uninstallMutation.isPending}
              onClick={() => setPendingUninstall(null)}
              type="button"
              variant="outline"
            >
              {t('plugins.actions.cancel')}
            </Button>
            <Button
              disabled={uninstallMutation.isPending || pendingUninstall === null}
              onClick={() => void confirmUninstall()}
              type="button"
              variant="destructive"
            >
              <Trash2 data-icon className="size-4" />
              {t('plugins.actions.confirmUninstall')}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </Section>
  )
}

function PluginCard({
  onOpenDetail,
  onReload,
  onToggle,
  onUninstall,
  operationPending,
  plugin,
}: {
  onOpenDetail: (pluginId: string) => void
  onReload: (pluginId: string) => void
  onToggle: (pluginId: string, enabled: boolean) => void
  onUninstall: (plugin: PluginSummary) => void
  operationPending: boolean
  plugin: PluginSummary
}) {
  const { t } = useTranslation('settings')
  const manageable = plugin.source === 'user'

  return (
    <article
      aria-label={plugin.name}
      className="rounded-md border border-border bg-background px-3 py-3"
    >
      <div className="grid gap-3 md:grid-cols-[minmax(0,1fr)_auto]">
        <div className="min-w-0">
          <div className="flex flex-wrap items-center gap-2">
            <h3 className="truncate font-semibold text-sm">{plugin.name}</h3>
            <span className="text-muted-foreground text-xs">{plugin.version}</span>
          </div>
          {plugin.description ? (
            <p className="mt-1 line-clamp-2 text-muted-foreground text-sm">{plugin.description}</p>
          ) : null}
          <div className="mt-3 flex flex-wrap gap-1.5">
            <Badge variant="outline">{t(`plugins.source.${plugin.source}`)}</Badge>
            <Badge variant="outline">{t(`plugins.trust.${plugin.trustLevel}`)}</Badge>
            <StatusBadge tone={stateBadgeTone(plugin.state)}>
              {pluginStateLabel(plugin.state, t)}
            </StatusBadge>
            {plugin.capabilities.map((capability) => (
              <Badge key={capabilityLabel(capability)} variant="outline">
                {capabilityLabel(capability)}
              </Badge>
            ))}
          </div>
          {plugin.warnings.length > 0 ? (
            <div className="mt-2 flex items-start gap-1.5 text-warning text-xs">
              <AlertTriangle className="mt-0.5 size-3.5 shrink-0" />
              <span>{plugin.warnings[0]}</span>
            </div>
          ) : null}
        </div>

        <div className="flex items-center gap-1 md:justify-end">
          {manageable ? (
            <Switch
              aria-label={t(
                plugin.enabled ? 'plugins.actions.disablePlugin' : 'plugins.actions.enablePlugin',
                {
                  name: plugin.name,
                },
              )}
              checked={plugin.enabled}
              disabled={operationPending}
              onCheckedChange={(enabled) => onToggle(plugin.id, enabled)}
            />
          ) : null}
          <IconButton
            icon={Settings}
            label={t('plugins.actions.viewDetailsFor', { name: plugin.name })}
            onClick={() => onOpenDetail(plugin.id)}
            type="button"
            variant="ghost"
          />
          {manageable ? (
            <>
              <IconButton
                disabled={operationPending}
                icon={RefreshCw}
                label={t('plugins.actions.reloadPlugin', { name: plugin.name })}
                onClick={() => onReload(plugin.id)}
                type="button"
                variant="ghost"
              />
              <IconButton
                disabled={operationPending}
                icon={Trash2}
                iconClassName="text-destructive"
                label={t('plugins.actions.uninstallPlugin', { name: plugin.name })}
                onClick={() => onUninstall(plugin)}
                type="button"
                variant="ghost"
              />
            </>
          ) : null}
        </div>
      </div>
    </article>
  )
}

function InstallCandidateDialog({
  error,
  installPending,
  onConfirm,
  onOpenChange,
  open,
  report,
  validating,
}: {
  error: boolean
  installPending: boolean
  onConfirm: () => void
  onOpenChange: (open: boolean) => void
  open: boolean
  report: PluginInstallReport | null
  validating: boolean
}) {
  const { t } = useTranslation('settings')
  const failureReason =
    report && !report.valid
      ? sanitizedMessage(report.reason, t('plugins.install.invalid'))
      : error
        ? t('plugins.install.validationError')
        : null

  return (
    <Dialog onOpenChange={onOpenChange} open={open}>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>{t('plugins.install.title')}</DialogTitle>
          <DialogDescription>{t('plugins.install.description')}</DialogDescription>
        </DialogHeader>

        {validating ? (
          <div className="text-muted-foreground text-sm">{t('plugins.install.validating')}</div>
        ) : null}

        {report?.valid ? (
          <div className="space-y-3">
            {report.summary ? (
              <div className="rounded-md border border-border bg-background p-3 text-sm">
                <div className="font-medium">{report.summary.name}</div>
                <div className="mt-1 text-muted-foreground">{report.summary.version}</div>
              </div>
            ) : null}
            {report.warnings.length > 0 ? (
              <div className="space-y-2">
                {report.warnings.map((warning) => (
                  <div
                    className="rounded-md border border-warning/30 bg-warning/5 px-3 py-2 text-sm text-warning"
                    key={warning}
                  >
                    {sanitizedMessage(warning, t('plugins.install.warning'))}
                  </div>
                ))}
              </div>
            ) : (
              <div className="rounded-md border border-border bg-background px-3 py-2 text-muted-foreground text-sm">
                {t('plugins.install.noWarnings')}
              </div>
            )}
          </div>
        ) : null}

        {failureReason ? <ErrorMessage>{failureReason}</ErrorMessage> : null}

        <DialogFooter>
          <Button
            disabled={installPending}
            onClick={() => onOpenChange(false)}
            type="button"
            variant="outline"
          >
            {t('plugins.actions.cancel')}
          </Button>
          {report?.valid ? (
            <Button disabled={installPending} onClick={onConfirm} type="button">
              {t('plugins.actions.confirmInstall')}
            </Button>
          ) : null}
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}

function PluginDetailDialog({
  onOpenChange,
  open,
  pluginId,
}: {
  onOpenChange: (open: boolean) => void
  open: boolean
  pluginId: string | null
}) {
  const { t } = useTranslation('settings')
  const commandClient = useCommandClient()
  const queryClient = useQueryClient()
  const detailQuery = useQuery<GetPluginDetailResponse>({
    enabled: pluginId !== null && open,
    queryKey: pluginQueryKeys.detail(pluginId),
    queryFn: () => getPluginDetail(pluginId ?? '', commandClient),
  })
  const updateConfigMutation = useMutation({
    mutationFn: ({ id, values }: { id: string; values: PluginConfigUpdate['values'] }) =>
      updatePluginConfig(id, values, commandClient),
    onSuccess: async (response) => {
      await queryClient.invalidateQueries({ queryKey: pluginQueryKeys.list() })
      await queryClient.invalidateQueries({
        queryKey: pluginQueryKeys.detail(response.pluginId ?? null),
      })
    },
  })
  const detail = detailQuery.data?.plugin ?? null

  return (
    <Dialog onOpenChange={onOpenChange} open={open}>
      <DialogContent className="max-h-[min(88vh,760px)] w-[min(calc(100vw-2rem),56rem)] overflow-y-auto">
        <DialogHeader>
          <DialogTitle>{detail?.summary.name ?? t('plugins.detail.title')}</DialogTitle>
          <DialogDescription>{t('plugins.detail.description')}</DialogDescription>
        </DialogHeader>

        {detailQuery.isLoading ? (
          <div className="text-muted-foreground text-sm">{t('plugins.detail.loading')}</div>
        ) : null}

        {detailQuery.isError ? <ErrorMessage>{t('plugins.detail.loadError')}</ErrorMessage> : null}

        {detail ? (
          <div className="space-y-5">
            <PluginDetailOverview detail={detail} />
            <PluginConfigForm
              detail={detail}
              manageable={detail.summary.source === 'user'}
              mutationPending={updateConfigMutation.isPending}
              mutationRejected={updateConfigMutation.isError}
              onSubmit={(values) => {
                updateConfigMutation.mutate({ id: detail.summary.id, values })
              }}
            />
          </div>
        ) : null}
      </DialogContent>
    </Dialog>
  )
}

function PluginDetailOverview({ detail }: { detail: PluginDetail }) {
  const { t } = useTranslation('settings')

  return (
    <div className="space-y-4">
      <section className="grid gap-3 text-sm sm:grid-cols-2">
        <InfoCell label={t('plugins.detail.origin')} value={manifestOriginLabel(detail)} />
        <InfoCell label={t('plugins.detail.hash')} value={hashBytes(detail.manifestHash)} />
        <InfoCell
          label={t('plugins.detail.state')}
          value={pluginStateLabel(detail.summary.state, t)}
        />
        <InfoCell
          label={t('plugins.detail.trust')}
          value={t(`plugins.trust.${detail.summary.trustLevel}`)}
        />
      </section>

      <section className="space-y-2">
        <h3 className="font-semibold text-sm">{t('plugins.detail.capabilities')}</h3>
        <div className="flex flex-wrap gap-1.5">
          {detail.registeredCapabilities.length > 0 ? (
            detail.registeredCapabilities.map((capability) => (
              <Badge key={capabilityLabel(capability)} variant="outline">
                {capabilityLabel(capability)}
              </Badge>
            ))
          ) : (
            <span className="text-muted-foreground text-sm">
              {t('plugins.detail.noCapabilities')}
            </span>
          )}
        </div>
      </section>

      {detail.summary.warnings.length > 0 ? (
        <section className="space-y-2">
          <h3 className="font-semibold text-sm">{t('plugins.detail.warnings')}</h3>
          {detail.summary.warnings.map((warning) => (
            <div
              className="rounded-md border border-warning/30 bg-warning/5 px-3 py-2 text-sm text-warning"
              key={warning}
            >
              {sanitizedMessage(warning, t('plugins.install.warning'))}
            </div>
          ))}
        </section>
      ) : null}

      {detail.failure ? (
        <ErrorMessage>{sanitizedMessage(detail.failure, t('plugins.detail.failure'))}</ErrorMessage>
      ) : null}

      <section className="space-y-2">
        <div className="flex items-center gap-2">
          <FileCode2 data-icon className="size-4 text-muted-foreground" />
          <h3 className="font-semibold text-sm">{t('plugins.detail.manifest')}</h3>
        </div>
        <pre className="max-h-64 overflow-auto rounded-md border border-border bg-background p-3 text-xs leading-5">
          {safeJsonStringify(detail.manifest)}
        </pre>
      </section>

      <section className="space-y-2">
        <h3 className="font-semibold text-sm">{t('plugins.detail.recentEvents')}</h3>
        {detail.recentEvents.length > 0 ? (
          <div className="flex flex-wrap gap-1.5">
            {detail.recentEvents.map((event) => (
              <Badge key={event} variant="outline">
                {sanitizedMessage(event, t('plugins.detail.event'))}
              </Badge>
            ))}
          </div>
        ) : (
          <div className="text-muted-foreground text-sm">{t('plugins.detail.noEvents')}</div>
        )}
      </section>
    </div>
  )
}

function PluginConfigForm({
  detail,
  manageable,
  mutationPending,
  mutationRejected,
  onSubmit,
}: {
  detail: PluginDetail
  manageable: boolean
  mutationPending: boolean
  mutationRejected: boolean
  onSubmit: (values: PluginConfigUpdate['values']) => void
}) {
  const { t } = useTranslation('settings')
  const fields = useMemo(
    () => configFieldsFromSchema(detail.configurationSchema),
    [detail.configurationSchema],
  )
  const [values, setValues] = useState<Record<string, EditableConfigValue>>({})

  useEffect(() => {
    setValues(initialConfigValues(fields, detail.config))
  }, [detail.config, fields])

  const editableFields = fields.filter((field) => !field.secret)
  const secretFields = fields.filter((field) => field.secret)

  function submit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault()
    if (!manageable) {
      return
    }
    const payload: PluginConfigUpdate['values'] = {}

    for (const field of editableFields) {
      payload[field.name] = values[field.name] ?? defaultConfigValue(field)
    }

    onSubmit(payload)
  }

  return (
    <form className="space-y-4 border-border border-t pt-5" onSubmit={submit}>
      <div>
        <h3 className="font-semibold text-sm">{t('plugins.config.title')}</h3>
        <p className="mt-1 text-muted-foreground text-sm">{t('plugins.config.description')}</p>
      </div>

      {fields.length === 0 ? (
        <div className="rounded-md border border-dashed border-border bg-background px-4 py-5 text-center text-muted-foreground text-sm">
          {t('plugins.config.empty')}
        </div>
      ) : null}

      {editableFields.length > 0 ? (
        <div className="grid gap-3 sm:grid-cols-2">
          {editableFields.map((field) => (
            <ConfigInput
              field={field}
              disabled={!manageable}
              key={field.name}
              onChange={(value) =>
                setValues((current) => ({
                  ...current,
                  [field.name]: value,
                }))
              }
              value={values[field.name] ?? defaultConfigValue(field)}
            />
          ))}
        </div>
      ) : null}

      {secretFields.length > 0 ? (
        <div className="grid gap-3 sm:grid-cols-2">
          {secretFields.map((field) => (
            <div
              className="rounded-md border border-border bg-background p-3 text-sm"
              key={field.name}
            >
              <div className="font-medium">{field.name}</div>
              <div className="mt-1 text-muted-foreground">{t('plugins.config.managedSecret')}</div>
            </div>
          ))}
        </div>
      ) : null}

      {mutationRejected ? <ErrorMessage>{t('plugins.config.saveError')}</ErrorMessage> : null}

      {fields.length > 0 && manageable ? (
        <div className="flex justify-end">
          <Button disabled={mutationPending} type="submit">
            {t('plugins.actions.saveConfig')}
          </Button>
        </div>
      ) : null}
    </form>
  )
}

function ConfigInput({
  disabled,
  field,
  onChange,
  value,
}: {
  disabled: boolean
  field: ConfigField
  onChange: (value: EditableConfigValue) => void
  value: EditableConfigValue
}) {
  const inputId = `plugin-config-${field.name.replace(/[^A-Za-z0-9_-]/g, '-')}`

  if (field.type === 'boolean') {
    return (
      <div className="flex items-center gap-2 rounded-md border border-border bg-background p-3 text-sm">
        <Checkbox
          aria-label={field.name}
          checked={Boolean(value)}
          disabled={disabled}
          id={inputId}
          onCheckedChange={(checked) => onChange(checked === true)}
        />
        <label className="font-medium" htmlFor={inputId}>
          {field.name}
        </label>
      </div>
    )
  }

  return (
    <label className="block space-y-2 text-sm" htmlFor={inputId}>
      <span className="font-medium">{field.name}</span>
      <Input
        aria-label={field.name}
        disabled={disabled}
        id={inputId}
        onChange={(event) => {
          if (field.type === 'number') {
            onChange(Number(event.target.value))
            return
          }
          onChange(event.target.value)
        }}
        type={field.type === 'number' ? 'number' : 'text'}
        value={String(value)}
      />
    </label>
  )
}

function InfoCell({ label, value }: { label: string; value: string }) {
  return (
    <div className="min-w-0 rounded-md border border-border bg-background p-3">
      <div className="text-muted-foreground">{label}</div>
      <div className="mt-1 break-words font-medium">{value}</div>
    </div>
  )
}

function ErrorMessage({ children }: { children: ReactNode }) {
  return (
    <div className="rounded-md border border-destructive/30 bg-destructive/5 px-3 py-2 text-destructive text-sm">
      {children}
    </div>
  )
}

function configFieldsFromSchema(schema: unknown): ConfigField[] {
  const root = objectRecord(schema)
  const properties = objectRecord(root?.properties)

  if (!properties) {
    return []
  }

  return Object.entries(properties).flatMap(([name, rawField]) => {
    const field = objectRecord(rawField)
    if (!field) {
      return []
    }

    const type = typeof field?.type === 'string' ? field.type : null

    if (!type || !supportedConfigTypes.has(type)) {
      return []
    }

    return [
      {
        name,
        secret: field.secret === true,
        type: type as ConfigFieldType,
      },
    ]
  })
}

function initialConfigValues(
  fields: ConfigField[],
  config: PluginDetail['config'],
): Record<string, EditableConfigValue> {
  const current = objectRecord(config)
  const values: Record<string, EditableConfigValue> = {}

  for (const field of fields) {
    if (field.secret) {
      continue
    }

    const currentValue = current?.[field.name]
    if (field.type === 'boolean') {
      values[field.name] = typeof currentValue === 'boolean' ? currentValue : false
    } else if (field.type === 'number') {
      values[field.name] = typeof currentValue === 'number' ? currentValue : 0
    } else {
      values[field.name] = typeof currentValue === 'string' ? currentValue : ''
    }
  }

  return values
}

function defaultConfigValue(field: ConfigField): EditableConfigValue {
  if (field.type === 'boolean') {
    return false
  }
  if (field.type === 'number') {
    return 0
  }
  return ''
}

function objectRecord(value: unknown): Record<string, unknown> | null {
  if (typeof value !== 'object' || value === null || Array.isArray(value)) {
    return null
  }

  return value as Record<string, unknown>
}

function capabilityLabel(capability: PluginRuntimeCapability): string {
  const kind = capability.kind === 'mcp_server' ? 'mcp' : capability.kind
  return capability.name ? `${kind}: ${capability.name}` : kind
}

function pluginStateLabel(
  state: PluginSummary['state'],
  t: ReturnType<typeof useTranslation<'settings'>>['t'],
): string {
  if (typeof state === 'string') {
    return t(`plugins.state.${state}`)
  }

  return t('plugins.state.disabled')
}

function stateBadgeTone(state: PluginSummary['state']): StatusBadgeProps['tone'] {
  if (state === 'activated') {
    return 'success'
  }
  if (state === 'failed' || state === 'rejected') {
    return 'destructive'
  }
  if (typeof state !== 'string') {
    return 'neutral'
  }
  return 'info'
}

function manifestOriginLabel(detail: PluginDetail): string {
  const origin = detail.manifestOrigin
  if ('file' in origin) {
    return origin.file.path
  }
  if ('cargo_extension' in origin) {
    return origin.cargo_extension.binary
  }
  return origin.remote_registry.endpoint
}

function hashBytes(bytes: number[]): string {
  return bytes.map((byte) => byte.toString(16).padStart(2, '0')).join('')
}

function safeJsonStringify(value: unknown): string {
  return JSON.stringify(redactDisplayJson(value), null, 2)
}

function redactDisplayJson(value: unknown): unknown {
  if (typeof value === 'string') {
    return hasSecretLikeText(value) ? '[redacted]' : value
  }

  if (Array.isArray(value)) {
    return value.map((item) => redactDisplayJson(item))
  }

  const record = objectRecord(value)
  if (!record) {
    return value
  }

  return Object.fromEntries(
    Object.entries(record).map(([key, item]) => [
      key,
      secretLikeKey(key) && typeof item === 'string' ? '[redacted]' : redactDisplayJson(item),
    ]),
  )
}

function sanitizedMessage(value: string | undefined, fallback: string): string {
  if (!value || hasSecretLikeText(value)) {
    return fallback
  }

  return value
}

function secretLikeKey(key: string): boolean {
  return /(?:api_?key|auth|authorization|bearer|password|secret|token)/i.test(key)
}

function hasSecretLikeText(value: string): boolean {
  return (
    /\bAuthorization:?\s*(?:Bearer|Basic)\s+\S+/i.test(value) ||
    /\b(?:api[_-]?key|token|secret|password)\b\s*[:=]\s*\S+/i.test(value) ||
    /\bsk-[A-Za-z0-9]{12,}/i.test(value) ||
    /\bgh[pousr]_[A-Za-z0-9_]{20,}/i.test(value)
  )
}
