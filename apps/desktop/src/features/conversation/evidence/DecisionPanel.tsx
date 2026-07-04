import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import { cn } from '@/shared/lib/utils'
import type { DecisionOption, DecisionRequestState } from '@/shared/tauri/commands'

export function DecisionPanel({
  conversationId,
  decision,
  onResolve,
}: {
  conversationId: string
  decision: DecisionRequestState
  onResolve?: (request: {
    conversationId: string
    requestId: string
    decision: 'approve' | 'deny'
    optionId: string
    confirmationText?: string
  }) => void
}) {
  const { t } = useTranslation('conversation')
  const [selectedOptionId, setSelectedOptionId] = useState<string | null>(null)
  const [confirmationText, setConfirmationText] = useState('')
  const canResolve = decision.status === 'pending' || decision.status === 'failed'
  const isSubmitting = decision.status === 'submitting'

  const riskLabel = t(`timeline.riskLevel.${decision.riskLevel}`)
  const operationLabel = t(`timeline.operation.${decision.operation}`)

  return (
    <div
      aria-live="polite"
      className="grid gap-3 rounded-md bg-muted px-3 py-3 text-sm"
      data-permission-request-id={decision.requestId}
      role="status"
    >
      {/* Header: operation + target + risk */}
      <div className="flex flex-wrap items-center gap-2">
        <span className="rounded bg-muted-foreground/10 px-1.5 py-0.5 font-medium text-xs">
          {operationLabel}
        </span>
        <span className="font-medium">{decision.target.label}</span>
        {decision.target.secondaryLabel ? (
          <span className="text-muted-foreground text-xs">{decision.target.secondaryLabel}</span>
        ) : null}
        <RiskBadge level={decision.riskLevel} label={riskLabel} />
      </div>

      {/* Reason */}
      {canResolve && decision.reason ? (
        <p className="text-muted-foreground text-xs">{decision.reason}</p>
      ) : null}

      {/* Data exposure */}
      <DataExposureInfo exposure={decision.dataExposure} />

      {/* Policy */}
      <div className="text-muted-foreground text-xs">
        {t('timeline.permissionMode')}: {decision.policy.mode}
        {decision.policy.rule ? ` · ${decision.policy.rule}` : ''}
        {decision.policy.sandbox ? ` · ${decision.policy.sandbox}` : ''}
      </div>

      {/* Decision options */}
      {canResolve && decision.decisionOptions.length > 0 ? (
        <div className="grid gap-2">
          <span className="text-muted-foreground text-xs">
            {t('timeline.selectApprovalOption')}
          </span>
          {decision.decisionOptions.map((option) => (
            <DecisionOptionButton
              key={option.id}
              option={option}
              selected={selectedOptionId === option.id}
              onSelect={(optionId) => setSelectedOptionId(optionId)}
            />
          ))}
        </div>
      ) : null}

      {/* Confirmation for high-risk */}
      {canResolve && decision.confirmation ? (
        <label className="grid gap-1">
          <span className="text-muted-foreground text-xs">
            {decision.confirmation.label}:{' '}
            <code className="rounded bg-background px-1 font-mono text-xs">
              {decision.confirmation.expectedText}
            </code>
          </span>
          <input
            aria-label={t('timeline.confirmationInput')}
            className="h-8 rounded-md border border-border bg-background px-2 text-foreground text-xs outline-none focus-visible:ring-2 focus-visible:ring-ring"
            onChange={(event) => setConfirmationText(event.currentTarget.value)}
            value={confirmationText}
          />
        </label>
      ) : null}

      {/* Action buttons */}
      {canResolve ? (
        <div className="flex gap-2">
          <button
            aria-label={t('timeline.deny')}
            className="rounded-md border border-border px-3 py-1.5 text-xs hover:bg-background focus-visible:ring-2 focus-visible:ring-ring"
            disabled={isSubmitting}
            onClick={() => {
              const denyOption = decision.decisionOptions.find((o) => o.decision === 'deny')
              onResolve?.({
                conversationId,
                requestId: decision.requestId,
                decision: 'deny',
                optionId: denyOption?.id ?? '',
              })
            }}
            type="button"
          >
            {t('timeline.deny')}
          </button>
          <button
            aria-label={t('timeline.approve')}
            className="rounded-md bg-primary px-3 py-1.5 text-primary-foreground text-xs focus-visible:ring-2 focus-visible:ring-ring disabled:cursor-not-allowed disabled:opacity-60"
            disabled={
              isSubmitting ||
              !selectedOptionId ||
              (decision.confirmation
                ? confirmationText !== decision.confirmation.expectedText
                : false)
            }
            onClick={() => {
              if (!selectedOptionId) return
              const approveOption = decision.decisionOptions.find(
                (o) => o.id === selectedOptionId && o.decision === 'approve',
              )
              if (!approveOption) return
              onResolve?.({
                conversationId,
                requestId: decision.requestId,
                decision: 'approve',
                optionId: selectedOptionId,
                ...(decision.confirmation ? { confirmationText } : {}),
              })
            }}
            type="button"
          >
            {selectedOptionId
              ? t('timeline.approveWithOption', {
                  label:
                    decision.decisionOptions.find((o) => o.id === selectedOptionId)?.label ??
                    t('timeline.approve'),
                })
              : t('timeline.approve')}
          </button>
        </div>
      ) : null}

      {/* Status display for non-pending */}
      {!canResolve ? (
        <p className="text-muted-foreground text-xs">
          {t(`timeline.permissionStatusLabel.${decision.status}`)}
        </p>
      ) : null}
    </div>
  )
}

function DecisionOptionButton({
  option,
  selected,
  onSelect,
}: {
  option: DecisionOption
  selected: boolean
  onSelect: (id: string) => void
}) {
  const { t } = useTranslation('conversation')
  const decisionLabel = option.decision === 'approve' ? t('timeline.approve') : t('timeline.deny')
  const lifetimeLabel = t(`timeline.lifetime.${option.lifetime}`)

  return (
    <button
      aria-pressed={selected}
      className={cn(
        'rounded-md border px-3 py-2 text-left text-xs transition-colors focus-visible:ring-2 focus-visible:ring-ring',
        selected ? 'border-primary bg-primary/10' : 'border-border hover:bg-background',
      )}
      onClick={() => onSelect(option.id)}
      type="button"
    >
      <div className="font-medium">
        {decisionLabel} — {option.label}
      </div>
      <div className="text-muted-foreground text-xs">
        {lifetimeLabel} · {option.matcher.label}
        {option.requiresConfirmation ? ` · ${t('timeline.requiresConfirmation')}` : ''}
      </div>
    </button>
  )
}

function RiskBadge({ level, label }: { level: string; label: string }) {
  const colors: Record<string, string> = {
    low: 'bg-green-100 text-green-800',
    medium: 'bg-yellow-100 text-yellow-800',
    high: 'bg-orange-100 text-orange-800',
    critical: 'bg-red-100 text-red-800',
  }
  return (
    <span className={cn('rounded px-1.5 py-0.5 font-medium text-xs', colors[level] ?? 'bg-muted')}>
      {label}
    </span>
  )
}

function DataExposureInfo({ exposure }: { exposure: DecisionRequestState['dataExposure'] }) {
  const { t } = useTranslation('conversation')
  const flags: string[] = []
  if (exposure.sendsWorkspaceData) flags.push(t('timeline.exposure.workspaceData'))
  if (exposure.sendsNetworkData) flags.push(t('timeline.exposure.networkData'))
  if (exposure.touchesPrivatePath) flags.push(t('timeline.exposure.privatePath'))
  if (exposure.secretRisk !== 'none') flags.push(t(`timeline.exposure.${exposure.secretRisk}`))

  if (flags.length === 0) return null

  return (
    <div className="flex flex-wrap gap-1">
      {flags.map((flag) => (
        <span
          key={flag}
          className="rounded bg-destructive/10 px-1.5 py-0.5 text-destructive text-xs"
        >
          {flag}
        </span>
      ))}
    </div>
  )
}
