import { Shield } from 'lucide-react'
import { useCallback, useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'

import type {
  AgentCapabilities,
  AgentCapabilityUnavailableReason,
  GetExecutionSettingsResponse,
  PermissionMode,
  ToolProfile,
} from '@/shared/tauri/commands'
import { getExecutionSettings, setExecutionSettings } from '@/shared/tauri/commands'
import { getCommandErrorMessage } from '@/shared/tauri/errors'
import { useCommandClient } from '@/shared/tauri/react'
import { Badge } from '@/shared/ui/badge'
import { Button } from '@/shared/ui/button'
import { Switch } from '@/shared/ui/switch'

const permissionModeOptions = [
  { value: 'default', labelKey: 'execution.mode.standard.label' },
  { value: 'auto', labelKey: 'execution.mode.auto.label' },
  { value: 'bypass_permissions', labelKey: 'execution.mode.bypass.label' },
] as const satisfies ReadonlyArray<{ value: PermissionMode; labelKey: string }>

const toolProfileOptions = [
  { value: 'minimal', labelKey: 'execution.toolProfile.minimal.label' },
  { value: 'coding', labelKey: 'execution.toolProfile.coding.label' },
  { value: 'full', labelKey: 'execution.toolProfile.full.label' },
] as const satisfies ReadonlyArray<{ value: ToolProfile; labelKey: string }>

const agentCapabilityOptions = [
  {
    availableKey: 'subagentsAvailable',
    descriptionKey: 'execution.agentCapabilities.subagents.description',
    enabledKey: 'subagentsEnabled',
    id: 'subagents',
    labelKey: 'execution.agentCapabilities.subagents.label',
  },
  {
    availableKey: 'agentTeamsAvailable',
    descriptionKey: 'execution.agentCapabilities.agentTeams.description',
    enabledKey: 'agentTeamsEnabled',
    id: 'agentTeams',
    labelKey: 'execution.agentCapabilities.agentTeams.label',
  },
  {
    availableKey: 'backgroundAgentsAvailable',
    descriptionKey: 'execution.agentCapabilities.backgroundAgents.description',
    enabledKey: 'backgroundAgentsEnabled',
    id: 'backgroundAgents',
    labelKey: 'execution.agentCapabilities.backgroundAgents.label',
  },
] as const

type AgentCapabilitySettings = Pick<
  AgentCapabilities,
  'agentTeamsEnabled' | 'backgroundAgentsEnabled' | 'subagentsEnabled'
>

const defaultAgentCapabilities: AgentCapabilities = {
  agentTeamsAvailable: false,
  agentTeamsEnabled: false,
  backgroundAgentsAvailable: false,
  backgroundAgentsEnabled: false,
  subagentsAvailable: false,
  subagentsEnabled: false,
  unavailableReasons: [],
}

export function ExecutionSettings() {
  const { t } = useTranslation('settings')
  const commandClient = useCommandClient()
  const [permissionMode, setPermissionMode] = useState<PermissionMode>('default')
  const [toolProfile, setToolProfile] = useState<ToolProfile>('full')
  const [contextCompressionTriggerPercent, setContextCompressionTriggerPercent] = useState(80)
  const [agentCapabilities, setAgentCapabilities] = useState(defaultAgentCapabilities)
  const [autoModeAvailable, setAutoModeAvailable] = useState(false)
  const [loading, setLoading] = useState(true)
  const [saving, setSaving] = useState(false)
  const [errorMessage, setErrorMessage] = useState<string | null>(null)

  const applySettings = useCallback((settings: GetExecutionSettingsResponse) => {
    setPermissionMode(settings.permissionMode)
    setToolProfile(settings.toolProfile)
    setContextCompressionTriggerPercent(Math.round(settings.contextCompressionTriggerRatio * 100))
    setAgentCapabilities(settings.agentCapabilities)
    setAutoModeAvailable(settings.autoModeAvailable)
  }, [])

  useEffect(() => {
    let cancelled = false

    async function loadSettings() {
      setLoading(true)
      setErrorMessage(null)

      try {
        const settings = await getExecutionSettings(commandClient)
        if (cancelled) {
          return
        }

        applySettings(settings)
      } catch (error) {
        if (!cancelled) {
          setErrorMessage(getCommandErrorMessage(error))
        }
      } finally {
        if (!cancelled) {
          setLoading(false)
        }
      }
    }

    void loadSettings()

    return () => {
      cancelled = true
    }
  }, [applySettings, commandClient])

  async function saveSettings(
    nextMode: PermissionMode,
    nextToolProfile: ToolProfile = toolProfile,
    nextContextCompressionTriggerPercent = contextCompressionTriggerPercent,
    nextAgentCapabilitySettings: AgentCapabilitySettings = getAgentCapabilitySettings(
      agentCapabilities,
    ),
  ) {
    const previousSettings = currentSettingsSnapshot({
      agentCapabilities,
      autoModeAvailable,
      contextCompressionTriggerPercent,
      permissionMode,
      toolProfile,
    })
    setSaving(true)
    setErrorMessage(null)

    try {
      const settings = await setExecutionSettings(
        {
          ...nextAgentCapabilitySettings,
          contextCompressionTriggerRatio: nextContextCompressionTriggerPercent / 100,
          permissionMode: nextMode,
          toolProfile: nextToolProfile,
        },
        commandClient,
      )
      applySettings(settings)
    } catch {
      try {
        applySettings(await getExecutionSettings(commandClient))
      } catch {
        applySettings(previousSettings)
      }
      setErrorMessage(t('execution.saveError'))
    } finally {
      setSaving(false)
    }
  }

  return (
    <section className="space-y-5 rounded-md border border-border bg-surface p-5">
      <div className="flex items-start gap-3">
        <div className="rounded-md border border-border bg-background p-2 text-muted-foreground">
          <Shield className="size-4" />
        </div>
        <div>
          <div className="flex flex-wrap items-center gap-2">
            <h2 className="font-semibold text-base">{t('execution.title')}</h2>
            <Badge variant="outline">{t('scope.globalDefaults')}</Badge>
          </div>
          <p className="mt-1 text-muted-foreground text-sm">{t('execution.description')}</p>
        </div>
      </div>

      {loading ? <p className="text-muted-foreground text-sm">{t('execution.loading')}</p> : null}
      {errorMessage ? <p className="text-destructive text-sm">{errorMessage}</p> : null}

      {!loading ? (
        <div className="space-y-5">
          <fieldset className="space-y-3">
            <legend className="font-medium text-sm">{t('execution.permissionMode.label')}</legend>
            {permissionModeOptions.map((option) => {
              const disabled =
                saving ||
                (option.value === 'auto' && !autoModeAvailable) ||
                permissionMode === option.value
              const descriptionKey =
                option.value === 'auto' && !autoModeAvailable
                  ? 'execution.mode.auto.unavailable'
                  : permissionModeDescriptionKey(option.value)

              return (
                <label
                  className="flex cursor-pointer items-start gap-3 rounded-md border border-border p-4"
                  key={option.value}
                >
                  <input
                    checked={permissionMode === option.value}
                    className="mt-1"
                    disabled={disabled}
                    name="permissionMode"
                    onChange={() => {
                      setPermissionMode(option.value)
                      void saveSettings(option.value)
                    }}
                    type="radio"
                    value={option.value}
                  />
                  <span className="space-y-1">
                    <span className="block font-medium text-sm">{t(option.labelKey)}</span>
                    <span className="block text-muted-foreground text-sm">{t(descriptionKey)}</span>
                  </span>
                </label>
              )
            })}
          </fieldset>

          <fieldset className="space-y-3">
            <legend className="font-medium text-sm">{t('execution.toolProfile.label')}</legend>
            {toolProfileOptions.map((option) => {
              const disabled = saving || toolProfile === option.value

              return (
                <label
                  className="flex cursor-pointer items-start gap-3 rounded-md border border-border p-4"
                  key={option.value}
                >
                  <input
                    checked={toolProfile === option.value}
                    className="mt-1"
                    disabled={disabled}
                    name="toolProfile"
                    onChange={() => {
                      setToolProfile(option.value)
                      void saveSettings(permissionMode, option.value)
                    }}
                    type="radio"
                    value={option.value}
                  />
                  <span className="space-y-1">
                    <span className="block font-medium text-sm">{t(option.labelKey)}</span>
                    <span className="block text-muted-foreground text-sm">
                      {t(toolProfileDescriptionKey(option.value))}
                    </span>
                  </span>
                </label>
              )
            })}
          </fieldset>

          <fieldset className="space-y-3">
            <legend className="font-medium text-sm">
              {t('execution.agentCapabilities.label')}
            </legend>
            <div className="space-y-3">
              {agentCapabilityOptions.map((option) => {
                const checked = agentCapabilities[option.enabledKey]
                const available = agentCapabilities[option.availableKey]
                const disabled = saving || !available
                const reasons = agentCapabilities.unavailableReasons.filter((reason) =>
                  reasonMatchesCapability(reason, option.id),
                )

                return (
                  <div
                    className="flex items-start justify-between gap-4 rounded-md border border-border p-4"
                    key={option.id}
                  >
                    <div className="space-y-1">
                      <label
                        className="block font-medium text-sm"
                        htmlFor={`execution-${option.id}`}
                      >
                        {t(option.labelKey)}
                      </label>
                      <p className="text-muted-foreground text-sm">{t(option.descriptionKey)}</p>
                      {!available && reasons.length > 0 ? (
                        <ul className="space-y-1 text-destructive text-sm">
                          {reasons.map((reason, index) => (
                            <li key={`${reason.type}-${index}`}>
                              {formatUnavailableReason(reason, t)}
                            </li>
                          ))}
                        </ul>
                      ) : null}
                    </div>
                    <Switch
                      checked={checked}
                      disabled={disabled}
                      id={`execution-${option.id}`}
                      onCheckedChange={(nextEnabled) => {
                        const nextAgentCapabilitySettings = {
                          ...getAgentCapabilitySettings(agentCapabilities),
                          [option.enabledKey]: nextEnabled,
                        }
                        setAgentCapabilities((current) => ({
                          ...current,
                          [option.enabledKey]: nextEnabled,
                        }))
                        void saveSettings(
                          permissionMode,
                          toolProfile,
                          contextCompressionTriggerPercent,
                          nextAgentCapabilitySettings,
                        )
                      }}
                    />
                  </div>
                )
              })}
            </div>
          </fieldset>

          <div className="space-y-2">
            <label
              className="block font-medium text-sm"
              htmlFor="context-compression-trigger-ratio"
            >
              {t('execution.contextCompressionTriggerRatio.label')}
            </label>
            <div className="flex items-center gap-2">
              <input
                className="h-9 w-24 rounded-sm border border-border bg-background px-3 text-sm outline-none focus-visible:ring-2 focus-visible:ring-ring"
                disabled={saving}
                id="context-compression-trigger-ratio"
                max={95}
                min={50}
                onChange={(event) => {
                  setContextCompressionTriggerPercent(Number(event.currentTarget.value))
                }}
                step={1}
                type="number"
                value={contextCompressionTriggerPercent}
              />
              <span className="text-muted-foreground text-sm">%</span>
            </div>
            <p className="text-muted-foreground text-sm">
              {t('execution.contextCompressionTriggerRatio.description')}
            </p>
          </div>
        </div>
      ) : null}

      <div className="flex justify-end">
        <Button
          disabled={loading || saving}
          onClick={() => void saveSettings(permissionMode)}
          type="button"
          variant="outline"
        >
          {saving ? t('execution.saving') : t('execution.save')}
        </Button>
      </div>
    </section>
  )
}

function toolProfileDescriptionKey(toolProfile: ToolProfile) {
  switch (toolProfile) {
    case 'minimal':
      return 'execution.toolProfile.minimal.description'
    case 'coding':
      return 'execution.toolProfile.coding.description'
    case 'full':
      return 'execution.toolProfile.full.description'
    default:
      return 'execution.toolProfile.custom.description'
  }
}

function permissionModeDescriptionKey(permissionMode: PermissionMode) {
  switch (permissionMode) {
    case 'default':
      return 'execution.mode.standard.description'
    case 'auto':
      return 'execution.mode.auto.description'
    case 'bypass_permissions':
      return 'execution.mode.bypass.description'
  }
}

function currentSettingsSnapshot({
  agentCapabilities,
  autoModeAvailable,
  contextCompressionTriggerPercent,
  permissionMode,
  toolProfile,
}: {
  agentCapabilities: AgentCapabilities
  autoModeAvailable: boolean
  contextCompressionTriggerPercent: number
  permissionMode: PermissionMode
  toolProfile: ToolProfile
}): GetExecutionSettingsResponse {
  return {
    agentCapabilities,
    autoModeAvailable,
    contextCompressionTriggerRatio: contextCompressionTriggerPercent / 100,
    permissionMode,
    toolProfile,
  }
}

function getAgentCapabilitySettings(agentCapabilities: AgentCapabilities): AgentCapabilitySettings {
  return {
    agentTeamsEnabled: agentCapabilities.agentTeamsEnabled,
    backgroundAgentsEnabled: agentCapabilities.backgroundAgentsEnabled,
    subagentsEnabled: agentCapabilities.subagentsEnabled,
  }
}

function reasonMatchesCapability(
  reason: AgentCapabilityUnavailableReason,
  capability: (typeof agentCapabilityOptions)[number]['id'],
) {
  if (reason.type === 'backgroundSupervisorUnavailable') {
    return capability === 'backgroundAgents'
  }

  return reason.capability === capability
}

function formatUnavailableReason(
  reason: AgentCapabilityUnavailableReason,
  t: ReturnType<typeof useTranslation<'settings'>>['t'],
) {
  switch (reason.type) {
    case 'notCompiled':
      return t('execution.agentCapabilities.unavailable.notCompiled')
    case 'runtimeStoreUnavailable':
      return t('execution.agentCapabilities.unavailable.runtimeStoreUnavailable', {
        message: reason.message,
      })
    case 'permissionRuntimeUnavailable':
      return t('execution.agentCapabilities.unavailable.permissionRuntimeUnavailable')
    case 'invalidAgentProfiles':
      return t('execution.agentCapabilities.unavailable.invalidAgentProfiles', {
        message: reason.message,
      })
    case 'backgroundSupervisorUnavailable':
      return t('execution.agentCapabilities.unavailable.backgroundSupervisorUnavailable', {
        message: reason.message,
      })
    case 'workspaceIsolationUnavailable':
      return t('execution.agentCapabilities.unavailable.workspaceIsolationUnavailable', {
        message: reason.message,
      })
  }
}
