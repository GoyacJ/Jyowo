import { useEffect, useMemo, useState } from 'react'
import { useTranslation } from 'react-i18next'

import type {
  DeleteProviderCapabilityRouteRequest,
  SaveProviderCapabilityRouteRequest,
} from '@/shared/tauri/commands'
import { Badge } from '@/shared/ui/badge'
import { Button } from '@/shared/ui/button'
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from '@/shared/ui/dialog'
import { RadioCard } from '@/shared/ui/radio-card-group'

import { costRiskLabel, executionLabel, routeKindLabel } from './CapabilityRoutesPanel'
import type { CapabilityRouteRow } from './model-settings-view-model'

type CapabilityRouteEditorDrawerProps = {
  open: boolean
  route: CapabilityRouteRow | null
  onOpenChange: (open: boolean) => void
  onSave: (request: SaveProviderCapabilityRouteRequest['route']) => void | Promise<void>
  onClear: (request: DeleteProviderCapabilityRouteRequest) => void | Promise<void>
}

export function CapabilityRouteEditorDrawer({
  onClear,
  onOpenChange,
  onSave,
  open,
  route,
}: CapabilityRouteEditorDrawerProps) {
  const { t } = useTranslation('settings')
  const [selectedConfigId, setSelectedConfigId] = useState<string | null>(null)

  useEffect(() => {
    setSelectedConfigId(
      route?.selectedTarget?.configId ?? route?.eligibleTargets[0]?.configId ?? null,
    )
  }, [route])

  const selectedTarget = useMemo(
    () => route?.eligibleTargets.find((target) => target.configId === selectedConfigId) ?? null,
    [route, selectedConfigId],
  )

  async function saveRoute() {
    if (!route || !selectedTarget) {
      return
    }

    await onSave({
      kind: route.kind,
      configId: selectedTarget.configId,
      providerId: selectedTarget.providerId,
      operationIds: selectedTarget.operationIds,
      enabled: true,
    })
  }

  async function clearRoute() {
    if (!route?.savedRoute) {
      return
    }

    await onClear({
      kind: route.savedRoute.kind,
      configId: route.savedRoute.configId,
      providerId: route.savedRoute.providerId,
    })
  }

  return (
    <Dialog onOpenChange={onOpenChange} open={open && route !== null}>
      {route ? (
        <DialogContent className="max-h-[calc(100vh-2rem)] w-[min(680px,92vw)] overflow-y-auto">
          <DialogHeader>
            <DialogTitle>
              {t('models.routes.editor.title', {
                kind: routeKindLabel(route.kind, t).toLowerCase(),
              })}
            </DialogTitle>
            <DialogDescription>{t('models.routes.editor.description')}</DialogDescription>
          </DialogHeader>

          <div className="space-y-4">
            <section className="space-y-2">
              <h3 className="font-medium text-sm">{t('models.routes.editor.eligible')}</h3>
              {route.eligibleTargets.length > 0 ? (
                <div className="space-y-2">
                  {route.eligibleTargets.map((target) => (
                    <RadioCard
                      aria-label={target.displayName}
                      checked={selectedConfigId === target.configId}
                      className="p-3"
                      key={target.configId}
                      name="capability-route-target"
                      onChange={() => setSelectedConfigId(target.configId)}
                      value={target.configId}
                    >
                      <span className="block font-medium">{target.displayName}</span>
                      <span className="block text-muted-foreground text-xs">
                        {target.providerDisplayName} / {target.modelId}
                      </span>
                      <span className="flex flex-wrap gap-2 text-xs">
                        <Badge variant="outline">{executionLabel(target.execution, t)}</Badge>
                        <Badge variant="outline">{costRiskLabel(target.costRisk, t)}</Badge>
                        <Badge variant="outline">
                          {t('models.routes.operationCount', {
                            count: target.operationIds.length,
                          })}
                        </Badge>
                      </span>
                      <span className="block text-muted-foreground text-xs">
                        {t('models.routes.editor.operationIds', {
                          operationIds: target.operationIds.join(', '),
                        })}
                      </span>
                    </RadioCard>
                  ))}
                </div>
              ) : (
                <p className="text-muted-foreground text-sm">{t('models.routes.empty')}</p>
              )}
            </section>

            {route.unavailableTargets.length > 0 ? (
              <section className="space-y-2">
                <h3 className="font-medium text-sm">{t('models.routes.editor.unavailable')}</h3>
                <div className="space-y-2">
                  {route.unavailableTargets.map((target) => (
                    <RadioCard
                      aria-label={target.displayName}
                      className="bg-muted p-3 text-muted-foreground"
                      disabled
                      key={`${target.configId}:${target.operationId}`}
                      name="capability-route-target"
                    >
                      <span className="block font-medium text-foreground">
                        {target.displayName}
                      </span>
                      <span className="block text-xs">{target.modelId}</span>
                      <span className="block text-xs">{target.reason}</span>
                    </RadioCard>
                  ))}
                </div>
              </section>
            ) : null}

            <div className="flex flex-wrap justify-end gap-2">
              <Button
                disabled={!route.savedRoute}
                onClick={() => void clearRoute()}
                type="button"
                variant="outline"
              >
                {t('models.routes.actions.clear')}
              </Button>
              <Button disabled={!selectedTarget} onClick={() => void saveRoute()} type="button">
                {t('models.routes.actions.save')}
              </Button>
            </div>
          </div>
        </DialogContent>
      ) : null}
    </Dialog>
  )
}
