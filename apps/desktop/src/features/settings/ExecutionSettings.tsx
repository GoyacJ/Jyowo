import { Shield } from 'lucide-react'
import { useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'

import type { PermissionMode } from '@/shared/tauri/commands'
import { getExecutionSettings, setExecutionSettings } from '@/shared/tauri/commands'
import { getCommandErrorMessage } from '@/shared/tauri/errors'
import { useCommandClient } from '@/shared/tauri/react'
import { Button } from '@/shared/ui/button'

const permissionModeOptions = [
  { value: 'default', labelKey: 'execution.mode.standard.label' },
  { value: 'auto', labelKey: 'execution.mode.auto.label' },
  { value: 'bypass_permissions', labelKey: 'execution.mode.bypass.label' },
] as const satisfies ReadonlyArray<{ value: PermissionMode; labelKey: string }>

const defaultAgentCapabilitySettings = {
  agentTeamsEnabled: false,
  backgroundAgentsEnabled: false,
  subagentsEnabled: false,
}

export function ExecutionSettings() {
  const { t } = useTranslation('settings')
  const commandClient = useCommandClient()
  const [permissionMode, setPermissionMode] = useState<PermissionMode>('default')
  const [contextCompressionTriggerPercent, setContextCompressionTriggerPercent] = useState(80)
  const [agentCapabilitySettings, setAgentCapabilitySettings] = useState(
    defaultAgentCapabilitySettings,
  )
  const [autoModeAvailable, setAutoModeAvailable] = useState(false)
  const [loading, setLoading] = useState(true)
  const [saving, setSaving] = useState(false)
  const [errorMessage, setErrorMessage] = useState<string | null>(null)
  const [savedMessage, setSavedMessage] = useState<string | null>(null)

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

        setPermissionMode(settings.permissionMode)
        setContextCompressionTriggerPercent(
          Math.round(settings.contextCompressionTriggerRatio * 100),
        )
        setAgentCapabilitySettings({
          agentTeamsEnabled: settings.agentCapabilities.agentTeamsEnabled,
          backgroundAgentsEnabled: settings.agentCapabilities.backgroundAgentsEnabled,
          subagentsEnabled: settings.agentCapabilities.subagentsEnabled,
        })
        setAutoModeAvailable(settings.autoModeAvailable)
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
  }, [commandClient])

  async function saveSettings(
    nextMode: PermissionMode,
    nextContextCompressionTriggerPercent = contextCompressionTriggerPercent,
  ) {
    setSaving(true)
    setErrorMessage(null)
    setSavedMessage(null)

    try {
      const settings = await setExecutionSettings(
        {
          ...agentCapabilitySettings,
          contextCompressionTriggerRatio: nextContextCompressionTriggerPercent / 100,
          permissionMode: nextMode,
        },
        commandClient,
      )
      setPermissionMode(settings.permissionMode)
      setContextCompressionTriggerPercent(Math.round(settings.contextCompressionTriggerRatio * 100))
      setAgentCapabilitySettings({
        agentTeamsEnabled: settings.agentCapabilities.agentTeamsEnabled,
        backgroundAgentsEnabled: settings.agentCapabilities.backgroundAgentsEnabled,
        subagentsEnabled: settings.agentCapabilities.subagentsEnabled,
      })
      setAutoModeAvailable(settings.autoModeAvailable)
      setSavedMessage(t('execution.saved'))
    } catch (error) {
      setErrorMessage(getCommandErrorMessage(error))
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
          <h2 className="font-semibold text-base">{t('execution.title')}</h2>
          <p className="mt-1 text-muted-foreground text-sm">{t('execution.description')}</p>
        </div>
      </div>

      {loading ? <p className="text-muted-foreground text-sm">{t('execution.loading')}</p> : null}
      {errorMessage ? <p className="text-destructive text-sm">{errorMessage}</p> : null}
      {savedMessage ? <p className="text-sm text-success">{savedMessage}</p> : null}

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
                  : (`execution.mode.${option.value === 'bypass_permissions' ? 'bypass' : option.value}.description` as const)

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
