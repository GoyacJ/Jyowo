import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import type { TFunction } from 'i18next'
import { Activity, ExternalLink, Plus, Save, Server, Trash2 } from 'lucide-react'
import { type ReactNode, useEffect, useMemo, useRef, useState } from 'react'
import { useFieldArray, useForm } from 'react-hook-form'
import { useTranslation } from 'react-i18next'
import { z } from 'zod'
import { formatTime } from '@/shared/formatters'
import type {
  McpConfigLayer,
  McpDiagnosticRecord,
  McpServerConfig,
  McpServerSummary,
  SaveMcpServerRequest,
} from '@/shared/tauri/commands'
import {
  clearMcpDiagnostics,
  getMcpServerConfig,
  listMcpDiagnostics,
  listMcpServers,
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
import { IconButton as SharedIconButton } from '@/shared/ui/icon-button'
import { Input } from '@/shared/ui/input'
import { Section, SectionDescription, SectionHeader, SectionTitle } from '@/shared/ui/section'
import { Select } from '@/shared/ui/select'

import { MCPServerCard } from './MCPServerCard'

const mcpServerQueryKeys = {
  all: ['mcp-servers'] as const,
  list: () => [...mcpServerQueryKeys.all, 'list', 'global'] as const,
}

const mcpDiagnosticQueryKeys = {
  all: ['mcp-diagnostics'] as const,
  list: (serverId: string | null) => [...mcpDiagnosticQueryKeys.all, 'list', serverId] as const,
}

type MCPServerFormValues = {
  args: Array<{ value: string }>
  bearerTokenEnvVar: string
  command: string
  displayName: string
  enabled: boolean
  env: Array<{ key: string; preserveExisting?: boolean; value: string }>
  headers: Array<{ key: string; preserveExisting?: boolean; value: string }>
  headersFromEnv: Array<{ envVar: string; key: string }>
  inheritEnv: Array<{ value: string }>
  required: boolean
  scope: 'global' | 'session' | 'agent'
  transportKind: 'stdio' | 'http'
  url: string
  workingDir: string
}

type McpDialogIdentity = {
  serverId: string | null
}

type McpSaveMutationVariables = {
  dialogGeneration: number
  dialogIdentity: McpDialogIdentity
  request: SaveMcpServerRequest
}

const defaultFormValues: MCPServerFormValues = {
  args: [],
  bearerTokenEnvVar: '',
  command: '',
  displayName: '',
  enabled: true,
  env: [],
  headers: [],
  headersFromEnv: [],
  inheritEnv: [],
  required: false,
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
  const [dialogIdentity, setDialogIdentityState] = useState<McpDialogIdentity | null>(null)
  const [editingServerId, setEditingServerId] = useState<string | null>(null)
  const [diagnosticServerId, setDiagnosticServerId] = useState<string | null>(null)
  const [diagnosticPlane, setDiagnosticPlane] = useState<'all' | 'settings' | 'task'>('all')
  const [diagnosticSubscriptionGeneration, setDiagnosticSubscriptionGeneration] = useState(0)
  const [configLoadError, setConfigLoadError] = useState(false)
  const [loadingConfigId, setLoadingConfigId] = useState<string | null>(null)
  const [saveErrorMessage, setSaveErrorMessage] = useState<string | null>(null)
  const configRequestGeneration = useRef(0)
  const diagnosticSubscriptionGenerationRef = useRef(0)
  const dialogIdentityRef = useRef<McpDialogIdentity | null>(null)
  const configRequestIdentity = useRef<{
    generation: number
    id: string
  } | null>(null)
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
    queryFn: () => listMcpServers('global', commandClient),
  })
  const diagnosticsQuery = useQuery({
    queryKey: mcpDiagnosticQueryKeys.list(diagnosticServerId),
    queryFn: () => listMcpDiagnostics(diagnosticServerId ?? undefined, commandClient),
    refetchOnWindowFocus: false,
    staleTime: Number.POSITIVE_INFINITY,
  })
  const saveMutation = useMutation({
    mutationFn: ({ request }: McpSaveMutationVariables) => saveMcpServer(request, commandClient),
    onSuccess: async (_, { dialogGeneration, dialogIdentity, request }) => {
      await queryClient.invalidateQueries({
        exact: true,
        queryKey: mcpServerQueryKeys.list(),
      })
      if (!isCurrentSaveMutation({ dialogGeneration, dialogIdentity, request })) {
        return
      }
      reset(defaultFormValues)
      closeDialog()
    },
    onError: (error, variables) => {
      if (!isCurrentSaveMutation(variables)) {
        return
      }
      setSaveErrorMessage(safeMcpSaveErrorMessage(error, t('mcp.saveError')))
    },
  })
  const deleteMutation = useMutation({
    mutationFn: (id: string) => commandClient.deleteMcpServer('global', id),
    onSuccess: async () => {
      await queryClient.invalidateQueries({ queryKey: mcpServerQueryKeys.all })
    },
  })
  const toggleMutation = useMutation({
    mutationFn: ({ enabled, id }: { enabled: boolean; id: string }) =>
      commandClient.setMcpServerEnabled('global', id, enabled),
    onSuccess: async () => {
      await queryClient.invalidateQueries({ queryKey: mcpServerQueryKeys.all })
    },
  })
  const restartMutation = useMutation({
    mutationFn: (id: string) => commandClient.restartMcpServer('global', id),
    onSuccess: async () => {
      await queryClient.invalidateQueries({ queryKey: mcpServerQueryKeys.all })
    },
  })
  const clearDiagnosticsMutation = useMutation({
    mutationFn: (serverId: string | null) =>
      clearMcpDiagnostics(serverId ?? undefined, commandClient),
    onSuccess: async (_, serverId) => {
      const nextSubscriptionGeneration = diagnosticSubscriptionGenerationRef.current + 1
      diagnosticSubscriptionGenerationRef.current = nextSubscriptionGeneration
      const queryKey = mcpDiagnosticQueryKeys.list(serverId)
      await queryClient.cancelQueries({ exact: true, queryKey })
      queryClient.setQueryData(queryKey, { events: [] })
      setDiagnosticSubscriptionGeneration(nextSubscriptionGeneration)
    },
  })
  const servers = serversQuery.data?.servers ?? []
  const diagnostics = diagnosticsQuery.data?.events ?? []
  const visibleDiagnostics = diagnostics.filter(
    (event) => diagnosticPlane === 'all' || event.plane === diagnosticPlane,
  )
  const pluginServers = servers.filter((server) => server.origin === 'plugin')
  const workspaceServers = servers.filter((server) => server.origin !== 'plugin')

  useEffect(() => {
    let active = true
    let subscriptionId: string | null = null
    let unlisten: (() => void) | undefined
    const subscriptionGeneration = diagnosticSubscriptionGeneration

    function isCurrentSubscription() {
      return active && diagnosticSubscriptionGenerationRef.current === subscriptionGeneration
    }

    async function subscribe() {
      try {
        const subscription = await commandClient.subscribeMcpDiagnostics({
          serverId: diagnosticServerId ?? undefined,
        })
        if (!isCurrentSubscription()) {
          void commandClient.unsubscribeMcpDiagnostics(subscription.subscriptionId)
          return
        }
        subscriptionId = subscription.subscriptionId
        const queryKey = mcpDiagnosticQueryKeys.list(diagnosticServerId)
        await queryClient.cancelQueries({ exact: true, queryKey })
        if (!isCurrentSubscription()) {
          return
        }
        queryClient.setQueryData(
          queryKey,
          (current: { events: McpDiagnosticRecord[] } | undefined) => ({
            events: mergeMcpDiagnosticEvents(current?.events ?? [], subscription.replayEvents),
          }),
        )
        const stopListening = await commandClient.listenMcpDiagnosticBatches((batch) => {
          if (!isCurrentSubscription() || batch.subscriptionId !== subscription.subscriptionId) {
            return
          }
          queryClient.setQueryData(
            queryKey,
            (current: { events: McpDiagnosticRecord[] } | undefined) => ({
              events: mergeMcpDiagnosticEvents(current?.events ?? [], batch.events).slice(-500),
            }),
          )
        })
        if (!isCurrentSubscription()) {
          stopListening()
          return
        }
        unlisten = stopListening
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
  }, [commandClient, diagnosticServerId, diagnosticSubscriptionGeneration, queryClient])

  const mcpServerFormSchema = useMemo(
    () =>
      z
        .object({
          args: z.array(z.object({ value: z.string() })),
          bearerTokenEnvVar: z.string(),
          command: z.string(),
          displayName: z.string().trim().min(1, t('mcp.errors.serverNameRequired')),
          enabled: z.boolean(),
          env: z.array(
            z.object({
              key: z.string(),
              preserveExisting: z.boolean().optional(),
              value: z.string(),
            }),
          ),
          headers: z.array(
            z.object({
              key: z.string(),
              preserveExisting: z.boolean().optional(),
              value: z.string(),
            }),
          ),
          headersFromEnv: z.array(z.object({ envVar: z.string(), key: z.string() })),
          inheritEnv: z.array(z.object({ value: z.string() })),
          required: z.boolean(),
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

    const identity = dialogIdentity
    if (!identity) {
      closeDialog()
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

    setSaveErrorMessage(null)
    try {
      await saveMutation.mutateAsync({
        dialogGeneration: configRequestGeneration.current,
        dialogIdentity: identity,
        request: payload.request,
      })
    } catch {
      // The rendered message is intentionally sanitized and does not use backend error text.
    }
  }

  function openCreateDialog() {
    invalidateConfigRequest()
    reset(defaultFormValues)
    setEditingServerId(null)
    setDialogIdentity({ serverId: null })
    setConfigLoadError(false)
    setLoadingConfigId(null)
    setSaveErrorMessage(null)
    setDialogOpen(true)
  }

  async function openConfigureDialog(server: McpServerSummary) {
    await openConfigDialog(server)
  }

  async function openConfigDialog(server: McpServerSummary) {
    const requestIdentity = {
      generation: invalidateConfigRequest(),
      id: server.id,
    }
    configRequestIdentity.current = requestIdentity
    reset({
      ...defaultFormValues,
      displayName: server.displayName,
      enabled: server.enabled,
      required: server.required,
      scope: server.scope,
      transportKind: server.transport === 'http' ? 'http' : 'stdio',
    })
    setEditingServerId(server.id)
    setDialogIdentity({ serverId: server.id })
    setConfigLoadError(false)
    setLoadingConfigId(server.id)
    setSaveErrorMessage(null)
    setDialogOpen(true)
    try {
      const payload = await getMcpServerConfig('global', server.id, commandClient)
      if (!isCurrentConfigRequest(requestIdentity)) {
        return
      }
      reset(mcpFormValuesFromConfig(payload.server))
      setEditingServerId(payload.server.id)
    } catch {
      if (!isCurrentConfigRequest(requestIdentity)) {
        return
      }
      setConfigLoadError(true)
    } finally {
      if (isCurrentConfigRequest(requestIdentity)) {
        configRequestIdentity.current = null
        setLoadingConfigId(null)
      }
    }
  }

  function invalidateConfigRequest(): number {
    configRequestGeneration.current += 1
    configRequestIdentity.current = null
    return configRequestGeneration.current
  }

  function setDialogIdentity(identity: McpDialogIdentity | null) {
    dialogIdentityRef.current = identity
    setDialogIdentityState(identity)
  }

  function isCurrentSaveMutation(variables: McpSaveMutationVariables) {
    return (
      variables.dialogGeneration === configRequestGeneration.current &&
      sameMcpDialogIdentity(dialogIdentityRef.current, variables.dialogIdentity)
    )
  }

  function isCurrentConfigRequest(request: NonNullable<typeof configRequestIdentity.current>) {
    const current = configRequestIdentity.current
    return current?.generation === request.generation && current.id === request.id
  }

  function closeDialog() {
    invalidateConfigRequest()
    setLoadingConfigId(null)
    setEditingServerId(null)
    setDialogIdentity(null)
    setSaveErrorMessage(null)
    setDialogOpen(false)
  }

  function handleDialogOpenChange(open: boolean) {
    if (!open) {
      closeDialog()
      return
    }
    setDialogOpen(true)
  }

  function updateDisplayName(value: string) {
    setValue('displayName', value)
  }

  return (
    <Section>
      <div className="flex flex-wrap items-start justify-between gap-3">
        <SectionHeader className="flex items-start gap-3">
          <div className="rounded-md border border-border bg-background p-2 text-muted-foreground">
            <Server className="size-4" />
          </div>
          <div>
            <div className="flex flex-wrap items-center gap-2">
              <SectionTitle>{t('mcp.title')}</SectionTitle>
              <Badge variant="outline">{t('scope.globalDefaults')}</Badge>
            </div>
            <SectionDescription>{t('mcp.description')}</SectionDescription>
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
        </SectionHeader>

        <Dialog open={dialogOpen} onOpenChange={handleDialogOpenChange}>
          <DialogTrigger asChild>
            <Button onClick={openCreateDialog} size="sm" type="button">
              <Plus className="size-4" />
              {t('mcp.addServer')}
            </Button>
          </DialogTrigger>
          <DialogContent className="max-h-[min(88vh,760px)] w-[min(calc(100vw-2rem),44rem)] overflow-y-auto">
            <DialogHeader>
              <DialogTitle>{t('mcp.dialogTitle')}</DialogTitle>
              <DialogDescription>
                {t('mcp.dialogDescription', {
                  layer: t('mcp.configLayer.global'),
                })}
              </DialogDescription>
            </DialogHeader>
            <form className="space-y-5" onSubmit={handleSubmit(submit)}>
              <div className="grid gap-4 md:grid-cols-2">
                <Field
                  fieldId="mcp-server-name"
                  label={t('mcp.serverName')}
                  message={errors.displayName?.message}
                >
                  <Input
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
                  label={t('mcp.runtimeScopeLabel')}
                  message={errors.scope?.message}
                >
                  <Select
                    className={inputClassName}
                    disabled={isSubmitting || isConfigLoading}
                    id="mcp-server-scope"
                    {...register('scope')}
                  >
                    <option value="global">{t('mcp.runtimeScope.global')}</option>
                    <option value="session">{t('mcp.runtimeScope.session')}</option>
                    <option value="agent">{t('mcp.runtimeScope.agent')}</option>
                  </Select>
                </Field>
                <label className="flex items-center gap-2 text-sm" htmlFor="mcp-server-required">
                  <input
                    className="size-4"
                    disabled={isSubmitting || isConfigLoading}
                    id="mcp-server-required"
                    type="checkbox"
                    {...register('required')}
                  />
                  <span>{t('mcp.requiredForRuns')}</span>
                </label>
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
                    <Input
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
                    <Input
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
                        <Input
                          aria-label={t('mcp.argument')}
                          className={inputClassName}
                          disabled={isSubmitting || isConfigLoading}
                          placeholder="mcp-server"
                          {...register(`args.${index}.value`)}
                        />
                        <SharedIconButton
                          className="shrink-0"
                          disabled={isSubmitting || isConfigLoading}
                          icon={Trash2}
                          label={t('mcp.removeArgument')}
                          onClick={() => argumentFields.remove(index)}
                          type="button"
                          variant="outline"
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
                        <Input
                          aria-label={t('mcp.inheritedEnvVar')}
                          className={inputClassName}
                          disabled={isSubmitting || isConfigLoading}
                          placeholder="PATH"
                          {...register(`inheritEnv.${index}.value`)}
                        />
                        <SharedIconButton
                          className="shrink-0"
                          disabled={isSubmitting || isConfigLoading}
                          icon={Trash2}
                          label={t('mcp.removeInheritedEnv')}
                          onClick={() => inheritEnvFields.remove(index)}
                          type="button"
                          variant="outline"
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
                          <Input
                            aria-label={t('mcp.envName')}
                            className={inputClassName}
                            disabled={isSubmitting || isConfigLoading}
                            placeholder="LOG_LEVEL"
                            {...register(`env.${index}.key`, {
                              onChange: () => setValue(`env.${index}.preserveExisting`, false),
                            })}
                          />
                          <Input
                            aria-label={t('mcp.envValue')}
                            className={inputClassName}
                            disabled={isSubmitting || isConfigLoading}
                            placeholder="info"
                            {...register(`env.${index}.value`, {
                              onChange: () => setValue(`env.${index}.preserveExisting`, false),
                            })}
                          />
                          <SharedIconButton
                            className="shrink-0"
                            disabled={isSubmitting || isConfigLoading}
                            icon={Trash2}
                            label={t('mcp.removeEnv')}
                            onClick={() => envFields.remove(index)}
                            type="button"
                            variant="outline"
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
                      <Input
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
                    <Input
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
                        <Input
                          aria-label={t('mcp.headerName')}
                          className={inputClassName}
                          disabled={isSubmitting || isConfigLoading}
                          placeholder="X-Workspace"
                          {...register(`headers.${index}.key`, {
                            onChange: () => setValue(`headers.${index}.preserveExisting`, false),
                          })}
                        />
                        <Input
                          aria-label={t('mcp.headerValue')}
                          className={inputClassName}
                          disabled={isSubmitting || isConfigLoading}
                          placeholder="jyowo"
                          {...register(`headers.${index}.value`, {
                            onChange: () => setValue(`headers.${index}.preserveExisting`, false),
                          })}
                        />
                        <SharedIconButton
                          className="shrink-0"
                          disabled={isSubmitting || isConfigLoading}
                          icon={Trash2}
                          label={t('mcp.removeHeader')}
                          onClick={() => headerFields.remove(index)}
                          type="button"
                          variant="outline"
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
                          <Input
                            aria-label={t('mcp.envHeaderName')}
                            className={inputClassName}
                            disabled={isSubmitting || isConfigLoading}
                            placeholder="X-Api-Key"
                            {...register(`headersFromEnv.${index}.key`)}
                          />
                          <Input
                            aria-label={t('mcp.envHeaderVariable')}
                            className={inputClassName}
                            disabled={isSubmitting || isConfigLoading}
                            placeholder="MCP_CONTEXT7_TOKEN"
                            {...register(`headersFromEnv.${index}.envVar`)}
                          />
                          <SharedIconButton
                            className="shrink-0"
                            disabled={isSubmitting || isConfigLoading}
                            icon={Trash2}
                            label={t('mcp.removeHeaderFromEnv')}
                            onClick={() => headerEnvFields.remove(index)}
                            type="button"
                            variant="outline"
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
                  onClick={closeDialog}
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

      <div className="grid items-start gap-5 lg:grid-cols-[minmax(0,1fr)_20rem]">
        <div className="min-w-0 space-y-5">
          {serversQuery.isError ? <ErrorMessage>{t('mcp.loadError')}</ErrorMessage> : null}

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
                onDelete={(server) => deleteMutation.mutate(server.id)}
                onOpenPlugin={onOpenPlugin}
                onRestart={(server) => restartMutation.mutate(server.id)}
                onToggle={(server, enabled) =>
                  toggleMutation.mutate({
                    enabled,
                    id: server.id,
                  })
                }
                servers={workspaceServers}
                title={t('mcp.serversGroup')}
                viewConfigLayer="global"
              />
              <ServerGroup
                empty={t('mcp.pluginsEmpty')}
                onConfigure={openConfigureDialog}
                onDelete={(server) => deleteMutation.mutate(server.id)}
                onOpenPlugin={onOpenPlugin}
                onRestart={(server) => restartMutation.mutate(server.id)}
                onToggle={(server, enabled) =>
                  toggleMutation.mutate({
                    enabled,
                    id: server.id,
                  })
                }
                servers={pluginServers}
                title={t('mcp.pluginsGroup')}
                viewConfigLayer="global"
              />
            </div>
          ) : null}
        </div>

        <aside
          aria-label={t('mcp.diagnostics.title')}
          className="overflow-hidden rounded-lg border border-border bg-background shadow-sm lg:sticky lg:top-0"
        >
          <div className="flex items-center justify-between gap-3 border-border border-b bg-muted/45 px-3.5 py-3">
            <div className="flex min-w-0 items-center gap-2.5">
              <div className="grid size-8 shrink-0 place-items-center rounded-md border border-border bg-surface text-muted-foreground shadow-sm">
                <Activity className="size-4" />
              </div>
              <div className="min-w-0">
                <h3 className="truncate font-semibold text-sm">{t('mcp.diagnostics.title')}</h3>
                <div className="text-muted-foreground text-xs">{t('scope.runtimeDiagnostics')}</div>
              </div>
            </div>
            <div className="flex shrink-0 items-center gap-1">
              <Badge variant="outline">{visibleDiagnostics.length}</Badge>
              <Button
                aria-label={t('mcp.diagnostics.clear')}
                disabled={clearDiagnosticsMutation.isPending || diagnostics.length === 0}
                onClick={() => clearDiagnosticsMutation.mutate(diagnosticServerId)}
                size="icon"
                type="button"
                variant="ghost"
              >
                <Trash2 className="size-4" />
              </Button>
            </div>
          </div>

          <div className="grid gap-2 border-border border-b bg-surface px-3 py-3">
            <Select
              aria-label={t('mcp.diagnostics.planeFilter')}
              className="h-8 w-full rounded-md border border-border bg-background px-2 text-sm"
              onChange={(event) =>
                setDiagnosticPlane(event.target.value as 'all' | 'settings' | 'task')
              }
              value={diagnosticPlane}
            >
              <option value="all">{t('mcp.diagnostics.allPlanes')}</option>
              <option value="settings">{t('mcp.diagnostics.plane.settings')}</option>
              <option value="task">{t('mcp.diagnostics.plane.task')}</option>
            </Select>
            <Select
              aria-label={t('mcp.diagnostics.filter')}
              className="h-8 w-full rounded-md border border-border bg-background px-2 text-sm"
              onChange={(event) => setDiagnosticServerId(event.target.value || null)}
              value={diagnosticServerId ?? ''}
            >
              <option value="">{t('mcp.diagnostics.allServers')}</option>
              {servers.map((server) => (
                <option key={`${server.configLayer}:${server.id}`} value={server.id}>
                  {server.displayName}
                </option>
              ))}
            </Select>
          </div>

          {diagnosticsQuery.isError ? (
            <div className="p-3">
              <ErrorMessage>{t('mcp.diagnostics.loadError')}</ErrorMessage>
            </div>
          ) : null}

          {diagnosticsQuery.isLoading ? (
            <div className="px-3.5 py-5 text-muted-foreground text-sm">
              {t('mcp.diagnostics.loading')}
            </div>
          ) : null}

          {!diagnosticsQuery.isLoading && visibleDiagnostics.length === 0 ? (
            <div className="px-4 py-8 text-center text-muted-foreground text-sm">
              <Activity className="mx-auto mb-2 size-5 opacity-45" />
              {t('mcp.diagnostics.empty')}
            </div>
          ) : null}

          {visibleDiagnostics.length > 0 ? (
            <div
              aria-live="polite"
              className="max-h-[34rem] divide-y divide-border overflow-y-auto"
            >
              {visibleDiagnostics
                .slice()
                .reverse()
                .map((event) => (
                  <DiagnosticRow event={event} key={event.id} servers={servers} />
                ))}
            </div>
          ) : null}
        </aside>
      </div>
    </Section>
  )
}

function ServerGroup({
  empty,
  onConfigure,
  onDelete,
  onOpenPlugin,
  onOverride,
  onRestart,
  onToggle,
  servers,
  title,
  viewConfigLayer,
}: {
  empty: string
  onConfigure: (server: McpServerSummary) => void
  onDelete: (server: McpServerSummary) => void
  onOpenPlugin?: (pluginId: string) => void
  onOverride?: (server: McpServerSummary) => void
  onRestart: (server: McpServerSummary) => void
  onToggle: (server: McpServerSummary, enabled: boolean) => void
  servers: McpServerSummary[]
  title: string
  viewConfigLayer: McpConfigLayer
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
              key={`${server.configLayer}:${server.id}`}
              onConfigure={onConfigure}
              onDelete={onDelete}
              onOpenPlugin={onOpenPlugin}
              onOverride={onOverride}
              onRestart={onRestart}
              onToggle={onToggle}
              server={server}
              viewConfigLayer={viewConfigLayer}
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
    <div className="bg-background px-3.5 py-3 transition-colors hover:bg-muted/35">
      <div className="flex items-center justify-between gap-2">
        <div className="flex items-center gap-1.5">
          <span className="size-1.5 rounded-full bg-current text-muted-foreground" />
          <span className="text-muted-foreground text-xs">
            {formatDiagnosticTime(event.timestamp)}
          </span>
        </div>
        <div className="flex items-center gap-1.5">
          <Badge variant="outline">{t(`mcp.diagnostics.plane.${event.plane}`)}</Badge>
        </div>
      </div>
      <div className="mt-2 font-medium text-sm leading-5">{event.summary}</div>
      <div className="mt-1.5 flex min-w-0 items-center gap-2">
        <Badge variant={severityVariant(event.severity)}>
          {t(`mcp.diagnostics.severity.${event.severity}`)}
        </Badge>
        <span className="min-w-0 truncate text-muted-foreground text-xs">
          {server?.displayName ?? t('mcp.diagnostics.unknownServer')}
        </span>
        <span aria-hidden="true" className="text-muted-foreground text-xs">
          ·
        </span>
        <span className="shrink-0 text-muted-foreground text-xs">
          {diagnosticEventTypeLabel(event.eventType, t)}
        </span>
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
    configLayer: 'global' as const,
    displayName: values.displayName.trim(),
    enabled: values.enabled,
    id: serverId,
    required: values.required,
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

function nameValueRows(rows: Array<{ key: string; preserveExisting?: boolean; value: string }>):
  | {
      ok: true
      values: Array<{ key: string; preserveExisting: true } | { key: string; value: string }>
    }
  | { ok: false } {
  const values: Array<{ key: string; preserveExisting: true } | { key: string; value: string }> = []
  for (const row of rows) {
    const key = row.key.trim()
    const value = row.value.trim()
    if (!key && !value) {
      continue
    }
    if (key && !value && row.preserveExisting) {
      values.push({ key, preserveExisting: true })
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
      enabled: server.enabled,
      headers: server.transport.headers.map((header) => ({
        key: header.key,
        preserveExisting: header.hasValue && header.value == null,
        value: header.value ?? '',
      })),
      headersFromEnv: server.transport.headersFromEnv.map((header) => ({ ...header })),
      required: server.required,
      scope: server.scope,
      transportKind: 'http',
      url: server.transport.url,
    }
  }

  if (server.transport.kind !== 'stdio') {
    throw new Error('In-process MCP server configuration is read-only')
  }

  return {
    ...defaultFormValues,
    args: server.transport.args.map((value) => ({ value })),
    command: server.transport.command,
    displayName: server.displayName,
    enabled: server.enabled,
    env: server.transport.env.map((item) => ({
      key: item.key,
      preserveExisting: item.hasValue && item.value == null,
      value: item.value ?? '',
    })),
    inheritEnv: server.transport.inheritEnv.map((value) => ({ value })),
    required: server.required,
    scope: server.scope,
    transportKind: 'stdio',
    workingDir: server.transport.workingDir ?? '',
  }
}

function mergeMcpDiagnosticEvents(
  current: McpDiagnosticRecord[],
  incoming: McpDiagnosticRecord[],
): McpDiagnosticRecord[] {
  const events = new Map(current.map((event) => [event.id, event]))
  for (const event of incoming) {
    events.set(event.id, event)
  }
  return [...events.values()]
}

function sameMcpDialogIdentity(
  current: McpDialogIdentity | null,
  submitted: McpDialogIdentity,
): boolean {
  return current?.serverId === submitted.serverId
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
  return formatTime(date)
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
