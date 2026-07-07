import { Settings2 } from 'lucide-react'
import { useTranslation } from 'react-i18next'

import { Badge } from '@/shared/ui/badge'
import { Button } from '@/shared/ui/button'

import type { CapabilityRouteRow, SectionState } from './model-settings-view-model'

type CapabilityRoutesPanelProps = {
  hasProjectScope?: boolean
  routeSection: SectionState<CapabilityRouteRow[]>
  onConfigure: (route: CapabilityRouteRow) => void
}

export function CapabilityRoutesPanel({
  hasProjectScope = true,
  onConfigure,
  routeSection,
}: CapabilityRoutesPanelProps) {
  const { t } = useTranslation('settings')

  if (routeSection.status === 'loading') {
    return (
      <section
        className="rounded-md border border-border bg-surface p-4 text-muted-foreground text-sm"
        role="status"
      >
        {t('models.routes.loading')}
      </section>
    )
  }

  if (routeSection.status === 'error') {
    return (
      <section
        className="rounded-md border border-border bg-surface p-4 text-muted-foreground text-sm"
        role="alert"
      >
        <div className="font-medium text-foreground">{t('models.routes.loadError')}</div>
        <div className="mt-1">{routeSection.safeMessage}</div>
      </section>
    )
  }

  if (routeSection.status === 'unavailable') {
    return (
      <section
        className="rounded-md border border-border bg-surface p-4 text-muted-foreground text-sm"
        role="status"
      >
        {t('models.routes.unavailable')}
      </section>
    )
  }

  if (routeSection.data.length === 0) {
    return (
      <section className="rounded-md border border-border bg-surface p-4">
        <div className="flex flex-wrap items-center gap-2">
          <h2 className="font-semibold text-base">{t('models.routes.title')}</h2>
          <Badge variant="outline">
            {hasProjectScope ? t('scope.projectOverrides') : t('scope.runtimeDiagnostics')}
          </Badge>
        </div>
        <p className="mt-2 text-muted-foreground text-sm">{t('models.routes.empty')}</p>
      </section>
    )
  }

  return (
    <section className="space-y-3" data-testid="capability-routes-panel">
      <div className="flex flex-wrap items-center justify-between gap-3">
        <div>
          <div className="flex flex-wrap items-center gap-2">
            <h2 className="font-semibold text-base">{t('models.routes.title')}</h2>
            <Badge variant="outline">
              {hasProjectScope ? t('scope.projectOverrides') : t('scope.runtimeDiagnostics')}
            </Badge>
          </div>
          <p className="mt-1 text-muted-foreground text-sm">{t('models.routes.description')}</p>
        </div>
      </div>

      <div className="overflow-x-auto rounded-md border border-border bg-surface">
        <table aria-label={t('models.routes.tableLabel')} className="w-full min-w-[760px] text-sm">
          <thead className="bg-muted/60 text-muted-foreground text-xs">
            <tr>
              <th className="px-3 py-2 text-left font-medium">{t('models.routes.columns.kind')}</th>
              <th className="px-3 py-2 text-left font-medium">
                {t('models.routes.columns.profile')}
              </th>
              <th className="px-3 py-2 text-left font-medium">
                {t('models.routes.columns.health')}
              </th>
              <th className="px-3 py-2 text-left font-medium">
                {t('models.routes.columns.execution')}
              </th>
              <th className="px-3 py-2 text-left font-medium">
                {t('models.routes.columns.costRisk')}
              </th>
              <th className="px-3 py-2 text-right font-medium">
                {t('models.routes.columns.actions')}
              </th>
            </tr>
          </thead>
          <tbody>
            {routeSection.data.map((route) => (
              <tr className="border-border border-t" key={route.kind}>
                <td className="px-3 py-3 font-medium">{routeKindLabel(route.kind, t)}</td>
                <td className="px-3 py-3">
                  {route.selectedTarget ? (
                    <div>
                      <div>{route.selectedTarget.displayName}</div>
                      <div className="text-muted-foreground text-xs">
                        {route.selectedTarget.providerDisplayName} / {route.selectedTarget.modelId}
                      </div>
                    </div>
                  ) : (
                    <Badge variant="outline">{t('models.routes.notConfigured')}</Badge>
                  )}
                </td>
                <td className="px-3 py-3">{routeHealthLabel(route, t)}</td>
                <td className="px-3 py-3">
                  {route.selectedTarget
                    ? executionLabel(route.selectedTarget.execution, t)
                    : t('models.routes.notConfigured')}
                </td>
                <td className="px-3 py-3">
                  {route.selectedTarget
                    ? costRiskLabel(route.selectedTarget.costRisk, t)
                    : t('models.routes.notConfigured')}
                </td>
                <td className="px-3 py-3 text-right">
                  <Button onClick={() => onConfigure(route)} type="button" variant="outline">
                    <Settings2 aria-hidden="true" data-icon />
                    {route.savedRoute
                      ? t('models.routes.actions.edit', { kind: routeKindLabel(route.kind, t) })
                      : t('models.routes.actions.configure', {
                          kind: routeKindLabel(route.kind, t).toLowerCase(),
                        })}
                  </Button>
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>
    </section>
  )
}

export function routeKindLabel(
  kind: CapabilityRouteRow['kind'],
  t: ReturnType<typeof useTranslation>['t'],
) {
  return t(`models.routes.kind.${kind}`)
}

export function executionLabel(
  execution: CapabilityRouteRow['eligibleTargets'][number]['execution'],
  t: ReturnType<typeof useTranslation>['t'],
) {
  return t(`models.routes.execution.${execution}`)
}

export function costRiskLabel(
  costRisk: CapabilityRouteRow['eligibleTargets'][number]['costRisk'],
  t: ReturnType<typeof useTranslation>['t'],
) {
  return t(`models.routes.costRisk.${costRisk}`)
}

function routeHealthLabel(route: CapabilityRouteRow, t: ReturnType<typeof useTranslation>['t']) {
  const health = route.selectedTarget?.health
  if (!health) {
    return t('models.routes.notConfigured')
  }
  if (health.status === 'never_checked') {
    return t('models.connectivity.neverChecked')
  }
  if (health.status === 'loading') {
    return t('models.summary.loadingMetric')
  }
  if (health.status === 'unavailable') {
    return t('models.unavailable')
  }
  return t(`models.connectivity.${health.status}`)
}
