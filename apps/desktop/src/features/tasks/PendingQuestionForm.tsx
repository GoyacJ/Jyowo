import { MessageCircleQuestion } from 'lucide-react'
import { useMemo, useState } from 'react'
import { useTranslation } from 'react-i18next'

import type {
  AskUserQuestion,
  AskUserQuestionAnswer,
  PendingQuestionProjection,
  TypedUlid,
} from '@/generated/daemon-protocol'
import { Button } from '@/shared/ui/button'
import { CheckboxCard, RadioCard } from '@/shared/ui/radio-card-group'
import { Textarea } from '@/shared/ui/textarea'
import { requireAcceptedCommand } from './task-command'
import type { TaskCommandExecutor } from './use-task-command-executor'

const optionClassName =
  'gap-3 bg-surface-raised px-3 py-2.5 text-left text-sm has-[:checked]:border-state-waiting/70 has-[:checked]:bg-row-muted'

export function PendingQuestionForm({
  executeCommand,
  pending,
  taskId,
}: {
  executeCommand: TaskCommandExecutor
  pending: PendingQuestionProjection
  taskId: TypedUlid
}) {
  const { t } = useTranslation('tasks')
  const [selected, setSelected] = useState<Record<string, string[]>>({})
  const [custom, setCustom] = useState<Record<string, string>>({})
  const [customOpen, setCustomOpen] = useState<Record<string, boolean>>({})
  const [submitting, setSubmitting] = useState<'answer' | 'decline' | null>(null)
  const [error, setError] = useState<string | null>(null)

  const answers = useMemo(
    () =>
      pending.questions.map<AskUserQuestionAnswer>((question) => {
        const acceptsText = question.options.length === 0 || customOpen[question.id]
        const text = acceptsText ? custom[question.id]?.trim() : undefined
        return {
          questionId: question.id,
          selectedOptionIds: selected[question.id] ?? [],
          ...(text ? { text } : {}),
        }
      }),
    [custom, customOpen, pending.questions, selected],
  )
  const answeredCount = pending.questions.filter((question) =>
    isQuestionComplete(question, selected, custom, customOpen),
  ).length
  const remainingCount = pending.questions.length - answeredCount
  const complete = remainingCount === 0

  function toggleOption(questionId: string, optionId: string, multiSelect: boolean) {
    if (!multiSelect) {
      setCustomOpen((current) => ({ ...current, [questionId]: false }))
    }
    setSelected((current) => {
      if (!multiSelect) return { ...current, [questionId]: [optionId] }
      const values = current[questionId] ?? []
      return {
        ...current,
        [questionId]: values.includes(optionId)
          ? values.filter((value) => value !== optionId)
          : [...values, optionId],
      }
    })
  }

  function toggleCustom(questionId: string, enabled: boolean, multiSelect: boolean) {
    setCustomOpen((current) => ({ ...current, [questionId]: enabled }))
    if (enabled && !multiSelect) {
      setSelected((current) => ({ ...current, [questionId]: [] }))
    }
  }

  async function respond(kind: 'answer' | 'decline') {
    if (submitting || (kind === 'answer' && !complete)) return
    setSubmitting(kind)
    setError(null)
    const operation = `resolve_question:${pending.requestId}:${pending.revision}:${kind}`
    try {
      const frame = await executeCommand(operation, (metadata) => ({
        metadata,
        questionRequestId: pending.requestId,
        requestRevision: pending.revision,
        response:
          kind === 'answer'
            ? { answers, status: 'answered' as const }
            : { status: 'declined' as const },
        taskId,
        type: 'resolve_question' as const,
      }))
      requireAcceptedCommand(frame, taskId)
    } catch (reason) {
      setSubmitting(null)
      setError(reason instanceof Error ? reason.message : String(reason))
    }
  }

  return (
    <section
      aria-labelledby={`question-${pending.requestId}`}
      className="mb-2 flex max-h-[min(60vh,36rem)] min-h-0 flex-col overflow-hidden rounded-xl border border-state-waiting/40 bg-artifact"
      data-artifact="true"
    >
      <p aria-live="polite" className="sr-only" role="status">
        {t('question.requestAnnouncement')}
      </p>
      <div className="flex min-h-10 shrink-0 items-center gap-2 border-border/70 border-b px-3">
        <MessageCircleQuestion aria-hidden="true" className="size-4 text-state-waiting" />
        <h2 className="font-medium text-sm" id={`question-${pending.requestId}`}>
          {t('question.title')}
        </h2>
        <span aria-live="polite" className="ml-auto text-muted-foreground text-xs">
          {t('question.progress', { answered: answeredCount, total: pending.questions.length })}
        </span>
      </div>
      <section
        aria-label={t('question.questionsLabel')}
        className="min-h-0 flex-1 space-y-5 overflow-y-auto overscroll-contain px-3 py-3"
      >
        {pending.questions.map((question, index) => {
          const values = selected[question.id] ?? []
          const textVisible = question.options.length === 0 || customOpen[question.id]
          const inputName = `question-${pending.requestId}-${index}`
          const OptionCard = question.multiSelect ? CheckboxCard : RadioCard
          return (
            <fieldset className="space-y-2" disabled={submitting !== null} key={question.id}>
              <legend className="font-medium text-sm">
                {question.header ? (
                  <span className="mr-2 text-muted-foreground text-xs">{question.header} </span>
                ) : null}
                {question.question}
              </legend>
              <p className="text-muted-foreground text-xs">
                {t('question.required')} · {questionModeLabel(question, t)}
              </p>
              {question.options.length > 0 ? (
                <div className="grid gap-2 sm:grid-cols-2">
                  {question.options.map((option) => {
                    const active = values.includes(option.id)
                    return (
                      <OptionCard
                        checked={active}
                        className={optionClassName}
                        disabled={submitting !== null}
                        key={option.id}
                        name={inputName}
                        onChange={() => toggleOption(question.id, option.id, question.multiSelect)}
                        value={option.id}
                      >
                        <span className="min-w-0">
                          <span className="block font-medium">{option.label}</span>
                          {option.description ? (
                            <span className="mt-0.5 block text-muted-foreground text-xs">
                              {option.description}
                            </span>
                          ) : null}
                        </span>
                      </OptionCard>
                    )
                  })}
                  {question.allowCustom ? (
                    <OptionCard
                      checked={Boolean(customOpen[question.id])}
                      className={optionClassName}
                      disabled={submitting !== null}
                      name={inputName}
                      onChange={(event) =>
                        toggleCustom(question.id, event.target.checked, question.multiSelect)
                      }
                      value="custom"
                    >
                      <span className="min-w-0">
                        <span className="block font-medium">{t('question.customOption')}</span>
                        <span className="mt-0.5 block text-muted-foreground text-xs">
                          {t('question.customOptionDescription')}
                        </span>
                      </span>
                    </OptionCard>
                  ) : null}
                </div>
              ) : null}
              {textVisible ? (
                <Textarea
                  aria-label={t(
                    question.options.length === 0
                      ? 'question.answerLabel'
                      : 'question.customAnswer',
                    { question: question.question },
                  )}
                  className="max-h-48 min-h-20"
                  maxLength={4096}
                  onChange={(event) =>
                    setCustom((current) => ({ ...current, [question.id]: event.target.value }))
                  }
                  placeholder={t(
                    question.options.length === 0
                      ? 'question.answerPlaceholder'
                      : 'question.customPlaceholder',
                  )}
                  rows={3}
                  value={custom[question.id] ?? ''}
                />
              ) : null}
            </fieldset>
          )
        })}
      </section>
      <div
        className="shrink-0 border-border/70 border-t px-3 py-2.5"
        data-testid="question-actions"
      >
        <div className="flex flex-wrap items-center gap-2">
          <p className="mr-auto text-muted-foreground text-xs" role="status">
            {complete ? t('question.ready') : t('question.remaining', { count: remainingCount })}
          </p>
          <Button
            disabled={!complete || submitting !== null}
            onClick={() => void respond('answer')}
            size="sm"
            type="button"
          >
            {submitting === 'answer' ? t('question.submitting') : t('question.submit')}
          </Button>
          <Button
            disabled={submitting !== null}
            onClick={() => void respond('decline')}
            size="sm"
            type="button"
            variant="outline"
          >
            {t('question.decline')}
          </Button>
        </div>
        {error ? (
          <p className="mt-2 text-destructive text-xs" role="alert">
            {error}
          </p>
        ) : null}
      </div>
    </section>
  )
}

function isQuestionComplete(
  question: AskUserQuestion,
  selected: Record<string, string[]>,
  custom: Record<string, string>,
  customOpen: Record<string, boolean>,
) {
  const hasText = Boolean(custom[question.id]?.trim())
  if (question.options.length === 0 || customOpen[question.id]) return hasText
  return (selected[question.id]?.length ?? 0) > 0
}

function questionModeLabel(
  question: AskUserQuestion,
  t: ReturnType<typeof useTranslation<'tasks'>>['t'],
) {
  if (question.options.length === 0) return t('question.writeAnswer')
  return t(question.multiSelect ? 'question.selectMany' : 'question.selectOne')
}
