import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import type { TFunction } from 'i18next'
import { Activity, ExternalLink, Plus, Power, Save, Server, Telescope, Trash2 } from 'lucide-react'
import { type ReactNode, useEffect, useMemo, useState } from 'react'
import { useFieldArray, useForm } from 'react-hook-form'
import { useTranslation } from 'react-i18next'
import { z } from 'zod'

import type {
  BrowserMcpPreset,
  McpDiagnosticRecord,
  McpServerConfig,
  McpServerSummary,
  SaveMcpServerRequest,
} from '@/shared/tauri/commands'
import {
  clearMcpDiagnostics,
  getMcpServerConfig,
  listBrowserMcpPresets,
  listMcpDiagnostics,
  listMcpServers,
  saveBrowserMcpPreset,
  saveMcpServer,
} from '@/shared/tauri/commands'
import { getCommandErrorMessage } from '@/shared/tauri/errors'
import { useCommandClient } from '@/shared/tauri/react'
import { Badge, type BadgeProps } from '@/shared/ui/badge'
import { Button } from '@/shared/ui/button'
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
  DialogTrigger,
} from '@/shared/ui/dialog'

import { MCPServerCard } from './MCPServerCard'

const mcpServerQueryKeys = {
  all: ['mcp-servers'] as const,
  list: () => [...mcpServerQueryKeys.all, 'list'] as const,
}

const mcpDiagnosticQueryKeys = {
  all: ['mcp-diagnostics'] as const,
  list: (serverId: string | null) => [...mcpDiagnosticQueryKeys.all, 'list', serverId] as const,
}

const browserMcpPresetQueryKeys = {
  all: ['browser-mcp-presets'] as const,
  list: () => [...browserMcpPresetQueryKeys.all, 'list'] as const,
}

type MCPServerFormValues = {
  args: Array<{ value: string }>
  bearerTokenEnvVar: string
  command: string
  displayName: string
  env: Array<{ key: string; value: string }>
  headers: Array<{ key: string; value: string }>
  headersFromEnv: Array<{ envVar: string; key: string }>
  inheritEnv: Array<{ value: string }>
  scope: 'global' | 'session' | 'agent'
  transportKind: 'stdio' | 'http'
  url: string
  workingDir: string
}

const defaultFormValues: MCPServerFormValues = {
  args: [],
  bearerTokenEnvVar: '',
  command: '',
  displayName: '',
  env: [],
  headers: [],
  headersFromEnv: [],
  inheritEnv: [],
  scope: 'global',
  transportKind: 'stdio',
  url: '',
  workingDir: '',
}

export function MCPManager({ onOpenPlugin }: { onOpenPlugin?: (pluginId: string) => void } = {}) {
  const { t } = useTranslation('settings')
  const commandClient = useCommandClient()
  const queryClient = useQueryClient()
  const [dialogOpen, setDialogOpen] = useState(false)
  const [editingServerId, setEditingServerId] = useState<string | null>(null)
  const [diagnosticServerId, setDiagnosticServerId] = useState<string | null>(null)
  const [configLoadError, setConfigLoadError] = useState(false)
  const [loadingConfigId, setLoadingConfigId] = useState<string | null>(null)
  const {
    control,
    formState: { errors, isSubmitting },
    handleSubmit,
    register,
    reset,
    setError,
    setValue,
    watch,
  } = useForm<MCPServerFormValues>({
    defaultValues: defaultFormValues,
  })
  const argumentFields = useFieldArray({ control, name: 'args' })
  const envFields = useFieldArray({ control, name: 'env' })
  const headerFields = useFieldArray({ control, name: 'headers' })
  const headerEnvFields = useFieldArray({ control, name: 'headersFromEnv' })
  const inheritEnvFields = useFieldArray({ control, name: 'inheritEnv' })
  const transportKind = watch('transportKind')
  const isConfigLoading = loadingConfigId !== null
  const serversQuery = useQuery({
    queryKey: mcpServerQueryKeys.list(),
    queryFn: () => listMcpServers(commandClient),
  })
  const diagnosticsQuery = useQuery({
    queryKey: mcpDiagnosticQueryKeys.list(diagnosticServerId),
    queryFn: () => listMcpDiagnostics(diagnosticServerId ?? undefined, commandClient),
  })
  const browserPresetsQuery = useQuery({
    queryKey: browserMcpPresetQueryKeys.list(),
    queryFn: () => listBrowserMcpPresets(commandClient),
  })
  const saveMutation = useMutation({
    mutationFn: (request: SaveMcpServerRequest) => saveMcpServer(request, commandClient),
    onSuccess: async () => {
      reset(defaultFormValues)
      setEditingServerId(null)
      setDialogOpen(false)
      await queryClient.invalidateQueries({ queryKey: mcpServerQueryKeys.all })
    },
  })
  const saveBrowserPresetMutation = useMutation({
    mutationFn: (preset: BrowserMcpPreset) =>
      saveBrowserMcpPreset({ enabled: !preset.enabled, presetId: preset.id }, commandClient),
    onSuccess: async () => {
      await Promise.all([
        queryClient.invalidateQueries({ queryKey: browserMcpPresetQueryKeys.all }),
        queryClient.invalidateQueries({ queryKey: mcpServerQueryKeys.all }),
      ])
    },
  })
  const deleteMutation = useMutation({
    mutationFn: (id: string) => commandClient.deleteMcpServer(id),
    onSuccess: async () => {
      await queryClient.invalidateQueries({ queryKey: mcpServerQueryKeys.all })
    },
  })
  const toggleMutation = useMutation({
    mutationFn: ({ enabled, id }: { enabled: boolean; id: string }) =>
      commandClient.setMcpServerEnabled(id, enabled),
    onSuccess: async () => {
      await queryClient.invalidateQueries({ queryKey: mcpServerQueryKeys.all })
    },
  })
  const restartMutation = useMutation({
    mutationFn: (id: string) => commandClient.restartMcpServer(id),
    onSuccess: async () => {
      await queryClient.invalidateQueries({ queryKey: mcpServerQueryKeys.all })
    },
  })
  const clearDiagnosticsMutation = useMutation({
    mutationFn: () => clearMcpDiagnostics(diagnosticServerId ?? undefined, commandClient),
    onSuccess: async () => {
      await queryClient.invalidateQueries({ queryKey: mcpDiagnosticQueryKeys.all })
    },
  })
  const servers = serversQuery.data?.servers ?? []
  const diagnostics = diagnosticsQuery.data?.events ?? []
  const browserPresets = browserPresetsQuery.data?.presets ?? []
  const workspaceServers = servers.filter((server) => server.manageable)
  const pluginServers = servers.filter((server) => !server.manageable)

  useEffect(() => {
    let active = true
    let subscriptionId: string | null = null
    let unlisten: (() => void) | undefined

    async function subscribe() {
      try {
        const subscription = await commandClient.subscribeMcpDiagnostics({
          serverId: diagnosticServerId ?? undefined,
        })
        if (!active) {
          void commandClient.unsubscribeMcpDiagnostics(subscription.subscriptionId)
          return
        }
        subscriptionId = subscription.subscriptionId
        queryClient.setQueryData(mcpDiagnosticQueryKeys.list(diagnosticServerId), {
          events: subscription.replayEvents,
        })
        unlisten = await commandClient.listenMcpDiagnosticBatches((batch) => {
          if (!active || batch.subscriptionId !== subscription.subscriptionId) {
            return
          }
          queryClient.setQueryData(
            mcpDiagnosticQueryKeys.list(diagnosticServerId),
            (current: { events: McpDiagnosticRecord[] } | undefined) => ({
              events: [...(current?.events ?? []), ...batch.events].slice(-500),
            }),
          )
        })
      } catch {
        // The query already renders a sanitized failure state.
      }
    }

    void subscribe()

    return () => {
      active = false
      unlisten?.()
      if (subscriptionId) {
        void commandClient.unsubscribeMcpDiagnostics(subscriptionId)
      }
    }
  }, [commandClient, diagnosticServerId, queryClient])

  const mcpServerFormSchema = useMemo(
    () =>
      z
        .object({
          args: z.array(z.object({ value: z.string() })),
          bearerTokenEnvVar: z.string(),
          command: z.string(),
          displayName: z.string().trim().min(1, t('mcp.errors.serverNameRequired')),
          env: z.array(z.object({ key: z.string(), value: z.string() })),
          headers: z.array(z.object({ key: z.string(), value: z.string() })),
          headersFromEnv: z.array(z.object({ envVar: z.string(), key: z.string() })),
          inheritEnv: z.array(z.object({ value: z.string() })),
          scope: z.enum(['global', 'session', 'agent']),
          transportKind: z.enum(['stdio', 'http']),
          url: z.string(),
          workingDir: z.string(),
        })
        .superRefine((values, context) => {
          if (values.transportKind === 'stdio' && values.command.trim().length === 0) {
            context.addIssue({
              code: 'custom',
              message: t('mcp.errors.commandRequired'),
              path: ['command'],
            })
          }
          if (values.transportKind === 'http') {
            if (values.url.trim().length === 0) {
              context.addIssue({
                code: 'custom',
                message: t('mcp.errors.urlRequired'),
                path: ['url'],
              })
            } else if (!/^https?:\/\//i.test(values.url.trim())) {
              context.addIssue({
                code: 'custom',
                message: t('mcp.errors.urlPattern'),
                path: ['url'],
              })
            }
          }
        }),
    [t],
  )

  async function submit(values: MCPServerFormValues) {
    const parsed = mcpServerFormSchema.safeParse(values)

    if (!parsed.success) {
      const handledFields = new Set<string>()
      for (const issue of parsed.error.issues) {
        const field = issue.path[0]
        if (typeof field !== 'string' || handledFields.has(field)) {
          continue
        }
        setError(field as keyof MCPServerFormValues, { message: issue.message, type: 'manual' })
        handledFields.add(field)
      }
      return
    }

    const payload = mcpServerPayload(
      parsed.data,
      editingServerId ??
        serverIdFromName(
          parsed.data.displayName,
          servers.map((server) => server.id),
        ),
      { rowIncomplete: t('mcp.errors.rowIncomplete') },
    )
    if (!payload.ok) {
      setError(payload.field, { message: payload.message, type: 'manual' })
      return
    }

    try {
      await saveMutation.mutateAsync(payload.request)
    } catch {
      // The rendered message is intentionally sanitized and does not use backend error text.
    }
  }

  function openCreateDialog() {
    reset(defaultFormValues)
    setEditingServerId(null)
    setConfigLoadError(false)
    setLoadingConfigId(null)
    setDialogOpen(true)
  }

  async function openConfigureDialog(server: McpServerSummary) {
    reset({
      ...defaultFormValues,
      displayName: server.displayName,
      scope: server.scope,
      transportKind: server.transport === 'http' ? 'http' : 'stdio',
    })
    setEditingServerId(server.id)
    setConfigLoadError(false)
    setLoadingConfigId(server.id)
    setDialogOpen(true)
    try {
      const payload = await getMcpServerConfig(server.id, commandClient)
      reset(mcpFormValuesFromConfig(payload.server))
      setEditingServerId(payload.server.id)
    } catch {
      setConfigLoadError(true)
    } finally {
      setLoadingConfigId(null)
    }
  }

  function updateDisplayName(value: string) {
    setValue('displayName', value)
  }

  const saveErrorMessage = saveMutation.isError
    ? safeMcpSaveErrorMessage(saveMutation.error, t('mcp.saveError'))
    : null

  return (
    <section className="space-y-5 rounded-md border border-border bg-surface p-5">
      <div className="flex flex-wrap items-start justify-between gap-3">
        <div className="flex items-start gap-3">
          <div className="rounded-md border border-border bg-background p-2 text-muted-foreground">
            <Server className="size-4" />
          </div>
          <div>
            <h2 className="font-semibold text-base">{t('mcp.title')}</h2>
            <p className="mt-1 text-muted-foreground text-sm">{t('mcp.description')}</p>
            <a
              className="mt-2 inline-flex items-center gap-1 text-muted-foreground text-xs hover:text-foreground"
              href="https://modelcontextprotocol.io"
              rel="noreferrer"
              target="_blank"
            >
              {t('mcp.docs')}
              <ExternalLink className="size-3" />
            </a>
          </div>
        </div>

        <Dialog open={dialogOpen} onOpenChange={setDialogOpen}>
          <DialogTrigger asChild>
            <Button onClick={openCreateDialog} size="sm" type="button">
              <Plus className="size-4" />
              {t('mcp.addServer')}
            </Button>
          </DialogTrigger>
          <DialogContent className="max-h-[min(88vh,760px)] w-[min(calc(100vw-2rem),44rem)] overflow-y-auto">
            <DialogHeader>
              <DialogTitle>{t('mcp.dialogTitle')}</DialogTitle>
              <DialogDescription>{t('mcp.dialogDescription')}</DialogDescription>
            </DialogHeader>
            <form className="space-y-5" onSubmit={handleSubmit(submit)}>
              <div className="grid gap-4 md:grid-cols-2">
                <Field
                  fieldId="mcp-server-name"
                  label={t('mcp.serverName')}
                  message={errors.displayName?.message}
                >
                  <input
                    className={inputClassName}
                    disabled={isSubmitting || isConfigLoading}
                    id="mcp-server-name"
                    placeholder={t('mcp.serverNamePlaceholder')}
                    {...register('displayName', {
                      onChange: (event) => updateDisplayName(String(event.target.value)),
                    })}
                  />
                </Field>
                <Field
                  fieldId="mcp-server-scope"
                  label={t('mcp.scope')}
                  message={errors.scope?.message}
                >
                  <select
                    className={inputClassName}
                    disabled={isSubmitting || isConfigLoading}
                    id="mcp-server-scope"
                    {...register('scope')}
                  >
                    <option value="global">{t('mcp.global')}</option>
                    <option value="session">{t('mcp.session')}</option>
                    <option value="agent">{t('mcp.agent')}</option>
                  </select>
                </Field>
                <div className="space-y-2 text-sm">
                  <span className="font-medium">{t('mcp.transport')}</span>
                  <div className="grid grid-cols-2 rounded-md border border-border bg-background p-1">
                    {(['stdio', 'http'] as const).map((kind) => (
                      <button
                        className={
                          transportKind === kind
                            ? 'rounded-sm bg-primary px-3 py-1.5 text-primary-foreground text-sm'
                            : 'rounded-sm px-3 py-1.5 text-muted-foreground text-sm hover:bg-muted'
                        }
                        disabled={isSubmitting || isConfigLoading}
                        key={kind}
                        onClick={() => setValue('transportKind', kind)}
                        type="button"
                      >
                        {t(`mcp.transportKind.${kind}`)}
                      </button>
                    ))}
                  </div>
                </div>
              </div>

              {transportKind === 'stdio' ? (
                <div className="grid gap-4 md:grid-cols-2">
                  <Field
                    fieldId="mcp-server-command"
                    label={t('mcp.command')}
                    message={errors.command?.message}
                  >
                    <input
                      className={inputClassName}
                      disabled={isSubmitting || isConfigLoading}
                      id="mcp-server-command"
                      placeholder="node"
                      {...register('command')}
                    />
                  </Field>
                  <Field
                    fieldId="mcp-server-working-dir"
                    label={t('mcp.workingDir')}
                    message={errors.workingDir?.message}
                  >
                    <input
                      className={inputClassName}
                      disabled={isSubmitting || isConfigLoading}
                      id="mcp-server-working-dir"
                      placeholder="."
                      {...register('workingDir')}
                    />
                  </Field>
                  <RepeatableField
                    addLabel={t('mcp.addArgument')}
                    disabled={isSubmitting || isConfigLoading}
                    label={t('mcp.arguments')}
                    message={formErrorMessage(errors.args)}
                    onAdd={() => argumentFields.append({ value: '' })}
                  >
                    {argumentFields.fields.map((field, index) => (
                      <div className="flex gap-2" key={field.id}>
                        <input
                          aria-label={t('mcp.argument')}
                          className={inputClassName}
                          disabled={isSubmitting || isConfigLoading}
                          placeholder="mcp-server"
                          {...register(`args.${index}.value`)}
                        />
                        <IconButton
                          disabled={isSubmitting || isConfigLoading}
                          label={t('mcp.removeArgument')}
                          onClick={() => argumentFields.remove(index)}
                        />
                      </div>
                    ))}
                  </RepeatableField>
                  <RepeatableField
                    addLabel={t('mcp.addInheritedEnv')}
                    disabled={isSubmitting || isConfigLoading}
                    label={t('mcp.inheritEnv')}
                    message={formErrorMessage(errors.inheritEnv)}
                    onAdd={() => inheritEnvFields.append({ value: '' })}
                  >
                    {inheritEnvFields.fields.map((field, index) => (
                      <div className="flex gap-2" key={field.id}>
                        <input
                          aria-label={t('mcp.inheritedEnvVar')}
                          className={inputClassName}
                          disabled={isSubmitting || isConfigLoading}
                          placeholder="GITHUB_TOKEN"
                          {...register(`inheritEnv.${index}.value`)}
                        />
                        <IconButton
                          disabled={isSubmitting || isConfigLoading}
                          label={t('mcp.removeInheritedEnv')}
                          onClick={() => inheritEnvFields.remove(index)}
                        />
                      </div>
                    ))}
                  </RepeatableField>
                  <div className="md:col-span-2">
                    <RepeatableField
                      addLabel={t('mcp.addEnv')}
                      disabled={isSubmitting || isConfigLoading}
                      label={t('mcp.env')}
                      message={formErrorMessage(errors.env)}
                      onAdd={() => envFields.append({ key: '', value: '' })}
                    >
                      {envFields.fields.map((field, index) => (
                        <div className="grid gap-2 md:grid-cols-[1fr_1fr_auto]" key={field.id}>
                          <input
                            aria-label={t('mcp.envName')}
                            className={inputClassName}
                            disabled={isSubmitting || isConfigLoading}
                            placeholder="LOG_LEVEL"
                            {...register(`env.${index}.key`)}
                          />
                          <input
                            aria-label={t('mcp.envValue')}
                            className={inputClassName}
                            disabled={isSubmitting || isConfigLoading}
                            placeholder="info"
                            {...register(`env.${index}.value`)}
                          />
                          <IconButton
                            disabled={isSubmitting || isConfigLoading}
                            label={t('mcp.removeEnv')}
                            onClick={() => envFields.remove(index)}
                          />
                        </div>
                      ))}
                    </RepeatableField>
                  </div>
                </div>
              ) : (
                <div className="grid gap-4 md:grid-cols-2">
                  <div className="md:col-span-2">
                    <Field
                      fieldId="mcp-server-url"
                      label={t('mcp.url')}
                      message={errors.url?.message}
                    >
                      <input
                        className={inputClassName}
                        disabled={isSubmitting || isConfigLoading}
                        id="mcp-server-url"
                        placeholder="https://mcp.example.com/mcp"
                        {...register('url')}
                      />
                    </Field>
                  </div>
                  <Field
                    fieldId="mcp-server-bearer-token-env-var"
                    label={t('mcp.bearerTokenEnvVar')}
                    message={errors.bearerTokenEnvVar?.message}
                  >
                    <input
                      className={inputClassName}
                      disabled={isSubmitting || isConfigLoading}
                      id="mcp-server-bearer-token-env-var"
                      placeholder="MCP_BEARER_TOKEN"
                      {...register('bearerTokenEnvVar')}
                    />
                  </Field>
                  <RepeatableField
                    addLabel={t('mcp.addHeader')}
                    disabled={isSubmitting || isConfigLoading}
                    label={t('mcp.headers')}
                    message={formErrorMessage(errors.headers)}
                    onAdd={() => headerFields.append({ key: '', value: '' })}
                  >
                    {headerFields.fields.map((field, index) => (
                      <div className="grid gap-2 md:grid-cols-[1fr_1fr_auto]" key={field.id}>
                        <input
                          aria-label={t('mcp.headerName')}
                          className={inputClassName}
                          disabled={isSubmitting || isConfigLoading}
                          placeholder="X-Workspace"
                          {...register(`headers.${index}.key`)}
                        />
                        <input
                          aria-label={t('mcp.headerValue')}
                          className={inputClassName}
                          disabled={isSubmitting || isConfigLoading}
                          placeholder="jyowo"
                          {...register(`headers.${index}.value`)}
                        />
                        <IconButton
                          disabled={isSubmitting || isConfigLoading}
                          label={t('mcp.removeHeader')}
                          onClick={() => headerFields.remove(index)}
                        />
                      </div>
                    ))}
                  </RepeatableField>
                  <div className="md:col-span-2">
                    <RepeatableField
                      addLabel={t('mcp.addHeaderFromEnv')}
                      disabled={isSubmitting || isConfigLoading}
                      label={t('mcp.headersFromEnv')}
                      message={formErrorMessage(errors.headersFromEnv)}
                      onAdd={() => headerEnvFields.append({ envVar: '', key: '' })}
                    >
                      {headerEnvFields.fields.map((field, index) => (
                        <div className="grid gap-2 md:grid-cols-[1fr_1fr_auto]" key={field.id}>
                          <input
                            aria-label={t('mcp.envHeaderName')}
                            className={inputClassName}
                            disabled={isSubmitting || isConfigLoading}
                            placeholder="X-Api-Key"
                            {...register(`headersFromEnv.${index}.key`)}
                          />
                          <input
                            aria-label={t('mcp.envHeaderVariable')}
                            className={inputClassName}
                            disabled={isSubmitting || isConfigLoading}
                            placeholder="MCP_CONTEXT7_TOKEN"
                            {...register(`headersFromEnv.${index}.envVar`)}
                          />
                          <IconButton
                            disabled={isSubmitting || isConfigLoading}
                            label={t('mcp.removeHeaderFromEnv')}
                            onClick={() => headerEnvFields.remove(index)}
                          />
                        </div>
                      ))}
                    </RepeatableField>
                  </div>
                </div>
              )}

              {isConfigLoading ? (
                <div className="rounded-md border border-border bg-background px-3 py-2 text-muted-foreground text-sm">
                  {t('mcp.loadingConfig')}
                </div>
              ) : null}

              {configLoadError ? <ErrorMessage>{t('mcp.configLoadError')}</ErrorMessage> : null}

              {saveErrorMessage ? (
                <div className="rounded-md border border-destructive/30 bg-destructive/5 px-3 py-2 text-destructive text-sm">
                  {saveErrorMessage}
                </div>
              ) : null}

              <DialogFooter>
                <Button
                  disabled={isSubmitting || isConfigLoading}
                  onClick={() => setDialogOpen(false)}
                  type="button"
                  variant="outline"
                >
                  {t('mcp.cancel')}
                </Button>
                <Button disabled={isSubmitting || isConfigLoading || configLoadError} type="submit">
                  <Save className="size-4" />
                  {isSubmitting ? t('mcp.saving') : t('mcp.save')}
                </Button>
              </DialogFooter>
            </form>
          </DialogContent>
        </Dialog>
      </div>

      {serversQuery.isError ? <ErrorMessage>{t('mcp.loadError')}</ErrorMessage> : null}

      <section className="space-y-3 border-border border-t pt-5">
        <div className="flex items-center gap-2">
          <Telescope className="size-4 text-muted-foreground" />
          <h3 className="font-semibold text-sm">{t('mcp.browserPresets.title')}</h3>
        </div>

        {browserPresetsQuery.isError ? (
          <ErrorMessage>{t('mcp.browserPresets.loadError')}</ErrorMessage>
        ) : null}

        {browserPresetsQuery.isLoading ? (
          <div className="text-muted-foreground text-sm">{t('mcp.browserPresets.loading')}</div>
        ) : null}

        {!browserPresetsQuery.isLoading && browserPresets.length === 0 ? (
          <div className="rounded-md border border-dashed border-border bg-background px-4 py-5 text-center text-muted-foreground text-sm">
            {t('mcp.browserPresets.empty')}
          </div>
        ) : null}

        {browserPresets.length > 0 ? (
          <div className="divide-y divide-border rounded-md border border-border bg-background">
            {browserPresets.map((preset) => (
              <div
                className="grid gap-3 px-3 py-3 md:grid-cols-[minmax(0,1fr)_auto] md:items-center"
                key={preset.id}
              >
                <div className="min-w-0">
                  <div className="flex flex-wrap items-center gap-2">
                    <h4 className="font-medium text-sm">{preset.displayName}</h4>
                    <Badge variant={preset.enabled ? 'secondary' : 'outline'}>
                      {preset.enabled
                        ? t('mcp.browserPresets.enabled')
                        : t('mcp.browserPresets.disabled')}
                    </Badge>
                  </div>
                  <p className="mt-1 text-muted-foreground text-sm">{preset.description}</p>
                </div>
                <Button
                  disabled={saveBrowserPresetMutation.isPending}
                  onClick={() => saveBrowserPresetMutation.mutate(preset)}
                  size="sm"
                  type="button"
                  variant="outline"
                >
                  {preset.enabled ? <Power className="size-4" /> : <Plus className="size-4" />}
                  {preset.enabled
                    ? t('mcp.browserPresets.disable', { name: preset.displayName })
                    : t('mcp.browserPresets.add', { name: preset.displayName })}
                </Button>
              </div>
            ))}
          </div>
        ) : null}

        {saveBrowserPresetMutation.isError ? (
          <ErrorMessage>{t('mcp.browserPresets.saveError')}</ErrorMessage>
        ) : null}
      </section>

      {serversQuery.isLoading ? (
        <div className="text-muted-foreground text-sm">{t('mcp.loading')}</div>
      ) : null}

      {!serversQuery.isLoading && servers.length === 0 ? (
        <div className="rounded-md border border-dashed border-border bg-background px-4 py-6 text-center text-muted-foreground text-sm">
          {t('mcp.empty')}
        </div>
      ) : null}

      {servers.length > 0 ? (
        <div className="space-y-5">
          <ServerGroup
            empty={t('mcp.groupEmpty')}
            onConfigure={openConfigureDialog}
            onDelete={(id) => deleteMutation.mutate(id)}
            onOpenPlugin={onOpenPlugin}
            onRestart={(id) => restartMutation.mutate(id)}
            onToggle={(id, enabled) => toggleMutation.mutate({ enabled, id })}
            servers={workspaceServers}
            title={t('mcp.serversGroup')}
          />
          <ServerGroup
            empty={t('mcp.pluginsEmpty')}
            onConfigure={openConfigureDialog}
            onDelete={(id) => deleteMutation.mutate(id)}
            onOpenPlugin={onOpenPlugin}
            onRestart={(id) => restartMutation.mutate(id)}
            onToggle={(id, enabled) => toggleMutation.mutate({ enabled, id })}
            servers={pluginServers}
            title={t('mcp.pluginsGroup')}
          />
        </div>
      ) : null}

      <section className="space-y-3 border-border border-t pt-5">
        <div className="flex flex-wrap items-center justify-between gap-3">
          <div className="flex items-center gap-2">
            <Activity className="size-4 text-muted-foreground" />
            <h3 className="font-semibold text-sm">{t('mcp.diagnostics.title')}</h3>
          </div>
          <div className="flex flex-wrap items-center gap-2">
            <select
              aria-label={t('mcp.diagnostics.filter')}
              className="h-8 rounded-md border border-border bg-background px-2 text-sm"
              onChange={(event) => setDiagnosticServerId(event.target.value || null)}
              value={diagnosticServerId ?? ''}
            >
              <option value="">{t('mcp.diagnostics.allServers')}</option>
              {servers.map((server) => (
                <option key={server.id} value={server.id}>
                  {server.displayName}
                </option>
              ))}
            </select>
            <Button
              disabled={clearDiagnosticsMutation.isPending || diagnostics.length === 0}
              onClick={() => clearDiagnosticsMutation.mutate()}
              size="sm"
              type="button"
              variant="outline"
            >
              <Trash2 className="size-4" />
              {t('mcp.diagnostics.clear')}
            </Button>
          </div>
        </div>

        {diagnosticsQuery.isError ? (
          <ErrorMessage>{t('mcp.diagnostics.loadError')}</ErrorMessage>
        ) : null}

        {diagnosticsQuery.isLoading ? (
          <div className="text-muted-foreground text-sm">{t('mcp.diagnostics.loading')}</div>
        ) : null}

        {!diagnosticsQuery.isLoading && diagnostics.length === 0 ? (
          <div className="rounded-md border border-dashed border-border bg-background px-4 py-5 text-center text-muted-foreground text-sm">
            {t('mcp.diagnostics.empty')}
          </div>
        ) : null}

        {diagnostics.length > 0 ? (
          <div className="max-h-72 overflow-auto rounded-md border border-border">
            {diagnostics
              .slice()
              .reverse()
              .map((event) => (
                <DiagnosticRow event={event} key={event.id} servers={servers} />
              ))}
          </div>
        ) : null}
      </section>
    </section>
  )
}

function ServerGroup({
  empty,
  onConfigure,
  onDelete,
  onOpenPlugin,
  onRestart,
  onToggle,
  servers,
  title,
}: {
  empty: string
  onConfigure: (server: McpServerSummary) => void
  onDelete: (id: string) => void
  onOpenPlugin?: (pluginId: string) => void
  onRestart: (id: string) => void
  onToggle: (id: string, enabled: boolean) => void
  servers: McpServerSummary[]
  title: string
}) {
  return (
    <section className="space-y-2">
      <div className="flex items-center justify-between gap-3">
        <h3 className="font-semibold text-sm">{title}</h3>
        <Badge variant="outline">{servers.length}</Badge>
      </div>
      {servers.length === 0 ? (
        <div className="rounded-md border border-dashed border-border bg-background px-3 py-4 text-muted-foreground text-sm">
          {empty}
        </div>
      ) : (
        <div className="space-y-2">
          {servers.map((server) => (
            <MCPServerCard
              key={server.id}
              onConfigure={onConfigure}
              onDelete={onDelete}
              onOpenPlugin={onOpenPlugin}
              onRestart={onRestart}
              onToggle={onToggle}
              server={server}
            />
          ))}
        </div>
      )}
    </section>
  )
}

function Field({
  children,
  fieldId,
  label,
  message,
}: {
  children: ReactNode
  fieldId: string
  label: string
  message?: string
}) {
  return (
    <div className="space-y-2 text-sm">
      <label className="block font-medium" htmlFor={fieldId}>
        {label}
      </label>
      {children}
      {message ? <span className="block text-destructive text-xs">{message}</span> : null}
    </div>
  )
}

function RepeatableField({
  addLabel,
  children,
  disabled,
  label,
  message,
  onAdd,
}: {
  addLabel: string
  children: ReactNode
  disabled: boolean
  label: string
  message?: string
  onAdd: () => void
}) {
  return (
    <div className="space-y-2 text-sm">
      <div className="flex items-center justify-between gap-3">
        <span className="font-medium">{label}</span>
        <Button disabled={disabled} onClick={onAdd} size="sm" type="button" variant="outline">
          <Plus className="size-4" />
          {addLabel}
        </Button>
      </div>
      <div className="space-y-2">{children}</div>
      {message ? <span className="block text-destructive text-xs">{message}</span> : null}
    </div>
  )
}

function IconButton({
  disabled,
  label,
  onClick,
}: {
  disabled: boolean
  label: string
  onClick: () => void
}) {
  return (
    <Button
      aria-label={label}
      className="shrink-0"
      disabled={disabled}
      onClick={onClick}
      size="icon"
      type="button"
      variant="outline"
    >
      <Trash2 className="size-4" />
    </Button>
  )
}

function ErrorMessage({ children }: { children: ReactNode }) {
  return (
    <div className="rounded-md border border-destructive/30 bg-destructive/5 px-3 py-2 text-destructive text-sm">
      {children}
    </div>
  )
}

function DiagnosticRow({
  event,
  servers,
}: {
  event: McpDiagnosticRecord
  servers: McpServerSummary[]
}) {
  const { t } = useTranslation('settings')
  const server = servers.find((server) => server.id === event.serverId)

  return (
    <div className="grid gap-2 border-border border-b bg-background px-3 py-2 last:border-b-0 md:grid-cols-[8rem_9rem_1fr]">
      <div className="text-muted-foreground text-xs">{formatDiagnosticTime(event.timestamp)}</div>
      <div className="flex items-center gap-2">
        <Badge variant={severityVariant(event.severity)}>
          {t(`mcp.diagnostics.severity.${event.severity}`)}
        </Badge>
      </div>
      <div className="min-w-0">
        <div className="truncate font-medium text-sm">{event.summary}</div>
        <div className="mt-0.5 flex flex-wrap gap-2 text-muted-foreground text-xs">
          <span>{server?.displayName ?? t('mcp.diagnostics.unknownServer')}</span>
          <span>{diagnosticEventTypeLabel(event.eventType, t)}</span>
        </div>
      </div>
    </div>
  )
}

function mcpServerPayload(
  values: MCPServerFormValues,
  serverId: string,
  messages: ParseMessages,
):
  | { ok: true; request: SaveMcpServerRequest }
  | { field: keyof MCPServerFormValues; message: string; ok: false } {
  const base = {
    displayName: values.displayName.trim(),
    enabled: true,
    id: serverId,
    scope: values.scope,
  }

  if (values.transportKind === 'stdio') {
    const env = nameValueRows(values.env)
    if (!env.ok) {
      return { field: 'env', message: messages.rowIncomplete, ok: false }
    }
    return {
      ok: true,
      request: {
        ...base,
        transport: {
          args: singleValueRows(values.args),
          command: values.command.trim(),
          env: env.values,
          inheritEnv: singleValueRows(values.inheritEnv),
          kind: 'stdio',
          workingDir: values.workingDir.trim() || undefined,
        },
      },
    }
  }

  const headers = nameValueRows(values.headers)
  if (!headers.ok) {
    return { field: 'headers', message: messages.rowIncomplete, ok: false }
  }
  const headersFromEnv = headerEnvRows(values.headersFromEnv)
  if (!headersFromEnv.ok) {
    return { field: 'headersFromEnv', message: messages.rowIncomplete, ok: false }
  }

  return {
    ok: true,
    request: {
      ...base,
      transport: {
        bearerTokenEnvVar: values.bearerTokenEnvVar.trim() || undefined,
        headers: headers.values,
        headersFromEnv: headersFromEnv.values,
        kind: 'http',
        url: values.url.trim(),
      },
    },
  }
}

function singleValueRows(rows: Array<{ value: string }>): string[] {
  return rows.map((row) => row.value.trim()).filter(Boolean)
}

function nameValueRows(
  rows: Array<{ key: string; value: string }>,
): { ok: true; values: Array<{ key: string; value: string }> } | { ok: false } {
  const values: Array<{ key: string; value: string }> = []
  for (const row of rows) {
    const key = row.key.trim()
    const value = row.value.trim()
    if (!key && !value) {
      continue
    }
    if (!key || !value) {
      return { ok: false }
    }
    values.push({ key, value })
  }
  return { ok: true, values }
}

function headerEnvRows(
  rows: Array<{ envVar: string; key: string }>,
): { ok: true; values: Array<{ envVar: string; key: string }> } | { ok: false } {
  const values: Array<{ envVar: string; key: string }> = []
  for (const row of rows) {
    const envVar = row.envVar.trim()
    const key = row.key.trim()
    if (!envVar && !key) {
      continue
    }
    if (!envVar || !key) {
      return { ok: false }
    }
    values.push({ envVar, key })
  }
  return { ok: true, values }
}

function mcpFormValuesFromConfig(server: McpServerConfig): MCPServerFormValues {
  if (server.transport.kind === 'http') {
    return {
      ...defaultFormValues,
      bearerTokenEnvVar: server.transport.bearerTokenEnvVar ?? '',
      displayName: server.displayName,
      headers: server.transport.headers.map((header) => ({ ...header })),
      headersFromEnv: server.transport.headersFromEnv.map((header) => ({ ...header })),
      scope: server.scope,
      transportKind: 'http',
      url: server.transport.url,
    }
  }

  return {
    ...defaultFormValues,
    args: server.transport.args.map((value) => ({ value })),
    command: server.transport.command,
    displayName: server.displayName,
    env: server.transport.env.map((item) => ({ ...item })),
    inheritEnv: server.transport.inheritEnv.map((value) => ({ value })),
    scope: server.scope,
    transportKind: 'stdio',
    workingDir: server.transport.workingDir ?? '',
  }
}

function slugFromName(value: string): string {
  return value
    .trim()
    .toLowerCase()
    .replace(/[^a-z0-9._-]+/g, '-')
    .replace(/^-+|-+$/g, '')
    .slice(0, 64)
}

function serverIdFromName(value: string, existingIds: string[]): string {
  const base = slugFromName(value) || `mcp-${hashString(value)}`
  const used = new Set(existingIds)
  if (!used.has(base)) {
    return base
  }
  for (let index = 2; index < 1000; index += 1) {
    const suffix = `-${index}`
    const candidate = `${base.slice(0, 64 - suffix.length)}${suffix}`
    if (!used.has(candidate)) {
      return candidate
    }
  }
  return `mcp-${hashString(`${value}:${existingIds.length}`)}`
}

function hashString(value: string): string {
  let hash = 0x811c9dc5
  for (let index = 0; index < value.length; index += 1) {
    hash ^= value.charCodeAt(index)
    hash = Math.imul(hash, 0x01000193)
  }
  return (hash >>> 0).toString(36)
}

function formatDiagnosticTime(value: string): string {
  const date = new Date(value)
  if (Number.isNaN(date.getTime())) {
    return value
  }
  return date.toLocaleTimeString([], { hour: '2-digit', minute: '2-digit', second: '2-digit' })
}

function severityVariant(severity: McpDiagnosticRecord['severity']): BadgeProps['variant'] {
  if (severity === 'error') {
    return 'destructive'
  }
  if (severity === 'warning') {
    return 'secondary'
  }
  return 'outline'
}

type ParseMessages = {
  rowIncomplete: string
}

function formErrorMessage(error: unknown): string | undefined {
  if (!error || typeof error !== 'object' || !('message' in error)) {
    return undefined
  }
  const message = (error as { message?: unknown }).message
  return typeof message === 'string' && message.length > 0 ? message : undefined
}

function safeMcpSaveErrorMessage(error: unknown, fallback: string): string {
  const message = getCommandErrorMessage(error)
  if (message === 'Unknown command error') {
    return fallback
  }
  const safeMessage = redactMcpSaveErrorMessage(message)
  return safeMessage.length > 0 ? safeMessage : fallback
}

function redactMcpSaveErrorMessage(value: string): string {
  return value
    .replaceAll(/(?:\/Users\/|\/home\/|\/private\/var\/|\/var\/folders\/)[^\s'",)]+/g, '<path>')
    .replaceAll(/[A-Za-z]:[\\/][^\s'",)]+/g, '<path>')
    .replaceAll(/\bbearer\s+\S+/gi, 'Bearer <redacted>')
    .replaceAll(
      /\B(--(?:api[_-]?key|token|secret|password))(=|\s+)\S+/gi,
      (_match, flag: string, separator: string) =>
        separator === '=' ? `${flag}=<redacted>` : `${flag} <redacted>`,
    )
    .replaceAll(
      /\b(?:api[_-]?key|token|secret|password)\s*[:=]\s*\S+/gi,
      (match) => `${match.split(/[:=]/, 1)[0]}=<redacted>`,
    )
    .replaceAll(
      /\b(?:ctx7sk|gh[pousr]|sk|rk|npm|lin_api|secret)_[A-Za-z0-9_-]{12,}\b/gi,
      '<redacted>',
    )
    .replaceAll(/\bctx7sk-[A-Za-z0-9-]{12,}\b/gi, '<redacted>')
    .replaceAll(/\bsk-(?:proj-)?[A-Za-z0-9_-]{12,}\b/gi, '<redacted>')
}

function diagnosticEventTypeLabel(eventType: string, t: TFunction<'settings'>): string {
  switch (eventType) {
    case 'connection_lost':
    case 'connection_recovered':
    case 'tools_changed':
    case 'resource_updated':
    case 'sampling':
    case 'elicitation_requested':
    case 'elicitation_resolved':
    case 'oauth_refresh':
    case 'tool_injected':
      return t(`mcp.diagnostics.eventType.${eventType}`)
    default:
      return eventType
  }
}

const inputClassName =
  'h-10 w-full rounded-md border border-border bg-background px-3 text-sm outline-none focus-visible:ring-2 focus-visible:ring-ring'
