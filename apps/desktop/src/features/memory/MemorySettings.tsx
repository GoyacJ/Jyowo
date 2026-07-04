import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { Save } from 'lucide-react'
import { useState } from 'react'
import { useTranslation } from 'react-i18next'

import {
  getMemorySettings,
  updateMemorySettings,
  type UpdateMemorySettingsRequest,
} from '@/shared/tauri/commands'
import { useCommandClient } from '@/shared/tauri/react'
import { Button } from '@/shared/ui/button'
import { Card, CardContent, CardHeader, CardTitle } from '@/shared/ui/card'
import { Input } from '@/shared/ui/input'
import { Label } from '@/shared/ui/label'
import { Switch } from '@/shared/ui/switch'

const settingsQueryKeys = {
  all: ['memory-settings'] as const,
}

export function MemorySettings() {
  const { t } = useTranslation('memory')
  const commandClient = useCommandClient()
  const queryClient = useQueryClient()
  const [saved, setSaved] = useState(false)

  const settingsQuery = useQuery({
    queryKey: settingsQueryKeys.all,
    queryFn: () => getMemorySettings(commandClient),
  })

  const updateMutation = useMutation({
    mutationFn: (req: UpdateMemorySettingsRequest) =>
      updateMemorySettings(req, commandClient),
    onSuccess: () => {
      setSaved(true)
      setTimeout(() => setSaved(false), 2000)
      queryClient.invalidateQueries({ queryKey: settingsQueryKeys.all })
    },
  })

  if (settingsQuery.isLoading) {
    return <div className="p-4 text-muted-foreground">{t('loading')}</div>
  }
  if (settingsQuery.isError) {
    return <div className="p-4 text-destructive">{t('errorLoading')}</div>
  }

  const settings = settingsQuery.data?.settings
  if (!settings) {
    return <div className="p-4 text-muted-foreground">{t('noSettings')}</div>
  }

  const handleToggle = (key: keyof typeof settings, value: boolean) => {
    updateMutation.mutate({
      tenantId: settingsQuery.data!.settings as any,
      settings: { ...settings, [key]: value },
    } as any)
  }

  return (
    <div className="space-y-4 p-4">
      <Card>
        <CardHeader>
          <CardTitle>{t('globalSettings')}</CardTitle>
        </CardHeader>
        <CardContent className="space-y-4">
          <div className="flex items-center justify-between">
            <Label htmlFor="use-memories">{t('useMemories')}</Label>
            <Switch
              id="use-memories"
              checked={settings.use_memories}
              onCheckedChange={(v) => handleToggle('use_memories', v)}
            />
          </div>
          <div className="flex items-center justify-between">
            <Label htmlFor="generate-memories">{t('generateMemories')}</Label>
            <Switch
              id="generate-memories"
              checked={settings.generate_memories}
              onCheckedChange={(v) => handleToggle('generate_memories', v)}
            />
          </div>
          <div className="flex items-center justify-between">
            <Label htmlFor="disable-ext-context">
              {t('disableGenerationWithExternalContext')}
            </Label>
            <Switch
              id="disable-ext-context"
              checked={settings.disable_generation_when_external_context_used}
              onCheckedChange={(v) =>
                handleToggle('disable_generation_when_external_context_used', v)
              }
            />
          </div>
        </CardContent>
      </Card>

      <Card>
        <CardHeader>
          <CardTitle>{t('limits')}</CardTitle>
        </CardHeader>
        <CardContent className="space-y-4">
          <div className="space-y-2">
            <Label htmlFor="max-records">{t('maxRecallRecordsPerTurn')}</Label>
            <Input
              id="max-records"
              type="number"
              value={settings.max_recall_records_per_turn}
              disabled
            />
          </div>
          <div className="space-y-2">
            <Label htmlFor="max-chars">{t('maxRecallCharsPerTurn')}</Label>
            <Input
              id="max-chars"
              type="number"
              value={settings.max_recall_chars_per_turn}
              disabled
            />
          </div>
          <div className="space-y-2">
            <Label htmlFor="max-bytes">{t('maxMemoryBytes')}</Label>
            <Input
              id="max-bytes"
              type="number"
              value={settings.max_memory_bytes}
              disabled
            />
          </div>
        </CardContent>
      </Card>

      {saved && (
        <div className="text-sm text-green-600">{t('settingsSaved')}</div>
      )}

      <Button
        onClick={() =>
          updateMutation.mutate({
            tenantId: undefined as any,
            settings,
          } as any)
        }
        disabled={updateMutation.isPending}
        className="w-full"
      >
        <Save className="mr-2 h-4 w-4" />
        {updateMutation.isPending ? t('saving') : t('saveSettings')}
      </Button>
    </div>
  )
}
