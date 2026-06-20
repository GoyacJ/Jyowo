import { useQuery } from '@tanstack/react-query'
import { Sparkles } from 'lucide-react'
import { useEffect, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'

import type { RunEvent } from '@/shared/events/run-event-schema'
import { MarkdownMessage } from '@/shared/markdown/MarkdownMessage'
import { getCommandErrorMessage } from '@/shared/tauri/errors'
import { useCommandClient } from '@/shared/tauri/react'
import { ArtifactSummary } from './ArtifactSummary'
import { Composer } from './Composer'
import { ConversationCanvas } from './ConversationCanvas'
import { ConversationMessage } from './ConversationMessage'
import type { ConversationRuntimeState } from './conversation-models'
import { DiffPreview } from './DiffPreview'
import { PlanBlock } from './PlanBlock'
import { ProgressBlock } from './ProgressBlock'
import { ReviewRequest } from './ReviewRequest'
import { type ConversationRecord, useConversation } from './use-conversation'

type OptimisticMessage = {
  confirmed: boolean
  knownUserMessageIds: Set<string>
  message: ConversationRuntimeState['messages'][number]
}

type ConversationWorkspaceProps = {
  conversationId?: string
}

let optimisticMessageSequence = 0

export function ConversationWorkspace({ conversationId }: ConversationWorkspaceProps) {
  const { i18n, t } = useTranslation('conversation')
  const commandClient = useCommandClient()
  const conversation = useConversation({ conversationId })
  const reviewContinuePendingRef = useRef(false)
  const [activeArtifactId, setActiveArtifactId] = useState<string | null>(null)
  const [localMessages, setLocalMessages] = useState<OptimisticMessage[]>([])
  const activityQuery = useQuery({
    enabled: Boolean(conversation.selectedConversationId),
    queryFn: () =>
      commandClient.listActivity({
        conversationId: conversation.selectedConversationId ?? '',
      }),
    queryKey: ['conversation-workspace-activity', conversation.selectedConversationId],
  })
  const artifactsQuery = useQuery({
    queryFn: () => commandClient.listArtifacts(),
    queryKey: ['conversation-workspace-artifacts'],
  })

  useEffect(() => {
    if (!conversation.conversation) {
      return
    }

    const currentConversation = conversation.conversation

    setLocalMessages((currentMessages) =>
      currentMessages.filter(
        (localMessage) =>
          !(
            localMessage.confirmed && hasNewPersistedUserMessage(currentConversation, localMessage)
          ),
      ),
    )
  }, [conversation.conversation])

  if (conversation.isLoading) {
    return (
      <section className="mx-auto flex min-h-full max-w-5xl flex-col">
        <p className="pt-4 text-muted-foreground text-sm">{t('loading')}</p>
      </section>
    )
  }

  if (conversation.error) {
    return (
      <section className="mx-auto flex min-h-full max-w-5xl flex-col">
        <h1 className="pt-4 font-semibold text-2xl tracking-normal">{t('unavailableTitle')}</h1>
        <p className="mt-3 text-destructive text-sm">
          {getCommandErrorMessage(conversation.error)}
        </p>
      </section>
    )
  }

  if (conversation.isEmpty || !conversation.conversation) {
    return (
      <section className="mx-auto flex min-h-full max-w-5xl flex-col">
        <h1 className="pt-4 font-semibold text-2xl tracking-normal">{t('emptyTitle')}</h1>
        <p className="mt-3 text-muted-foreground text-sm">{t('emptyDescription')}</p>
      </section>
    )
  }

  const currentConversation = conversation.conversation
  const runtimeState = toRuntimeState(
    currentConversation,
    localMessages,
    activityQuery.data?.events ?? [],
    artifactsQuery.data?.artifacts ?? [],
    {
      locale: i18n.language,
      continueLabel: t('continue'),
      now: t('now'),
      reviewGeneratedFoundation: t('reviewGeneratedFoundation'),
      userAuthor: t('userAuthor'),
    },
  )
  const completedPlanCount = runtimeState.planItems.filter(
    (item) => item.status === 'completed',
  ).length
  const activeActivity = runtimeState.activityItems[0]
  const workMessageId = getLatestAssistantMessageId(runtimeState.messages)
  const hasMessages = runtimeState.messages.length > 0

  function submitMessage(message: string) {
    const optimisticMessage = createOptimisticMessage(currentConversation, message, {
      now: t('now'),
      userAuthor: t('userAuthor'),
    })
    setLocalMessages((currentMessages) => [...currentMessages, optimisticMessage])
    const submitPromise = conversation
      .submitPrompt(message)
      .then(() => {
        setLocalMessages((currentMessages) =>
          currentMessages.map((currentMessage) =>
            currentMessage.message.id === optimisticMessage.message.id
              ? { ...currentMessage, confirmed: true }
              : currentMessage,
          ),
        )
      })
      .catch(() => {
        // The composer renders submit errors without replacing the loaded conversation.
      })

    void submitPromise
    return submitPromise
  }

  function submitReviewContinue(message: string) {
    if (conversation.isSubmitting || reviewContinuePendingRef.current) {
      return
    }

    reviewContinuePendingRef.current = true
    void submitMessage(message).finally(() => {
      reviewContinuePendingRef.current = false
    })
  }

  return (
    <section className="mx-auto grid h-full min-h-0 w-full max-w-[900px] grid-rows-[minmax(0,1fr)_auto]">
      <div className="min-h-0 overflow-auto pr-1">
        {hasMessages ? (
          <ConversationCanvas title={currentConversation.title}>
            {runtimeState.messages.map((message) => (
              <ConversationMessage
                author={message.author}
                avatar={message.avatar}
                body={toMessageBodyNode(message, { stripPlanBlock: message.id === workMessageId })}
                elementId={getConversationMessageElementId(message.id)}
                key={message.id}
                time={message.time}
                tone={message.tone === 'assistant' ? 'assistant' : undefined}
              >
                {message.id === workMessageId ? (
                  <>
                    {runtimeState.planItems.length > 0 ? (
                      <PlanBlock
                        completedCount={completedPlanCount}
                        items={runtimeState.planItems.map(toPlanBlockItem)}
                        totalCount={runtimeState.planItems.length}
                      />
                    ) : null}

                    {activeActivity ? (
                      <ProgressBlock
                        label={activeActivity.label}
                        status={activeActivity.status}
                        time={activeActivity.time}
                      />
                    ) : null}

                    {runtimeState.diffPreview ? (
                      <DiffPreview
                        addedLineCount={runtimeState.diffPreview.addedLineCount}
                        filename={runtimeState.diffPreview.filename}
                        lines={runtimeState.diffPreview.lines.map((line) => line.content)}
                        maxVisibleLines={3}
                      />
                    ) : null}

                    {runtimeState.artifacts.length > 0 ? (
                      <ArtifactSummary
                        activeArtifactId={activeArtifactId ?? undefined}
                        artifacts={runtimeState.artifacts}
                        onOpenArtifact={setActiveArtifactId}
                        onOpenSource={scrollToMessage}
                      />
                    ) : null}

                    {runtimeState.reviewRequest ? (
                      <ReviewRequest
                        continueActionLabel={runtimeState.reviewRequest.continueActionLabel}
                        disabled={conversation.isSubmitting || reviewContinuePendingRef.current}
                        onContinue={() =>
                          submitReviewContinue(
                            runtimeState.reviewRequest?.continuePrompt ?? t('continue'),
                          )
                        }
                        title={runtimeState.reviewRequest.title}
                      />
                    ) : null}
                  </>
                ) : null}
              </ConversationMessage>
            ))}
          </ConversationCanvas>
        ) : (
          <EmptyConversation
            disabled={conversation.isSubmitting}
            onPickExample={submitMessage}
            title={currentConversation.title}
          />
        )}
      </div>

      <div className="pt-4">
        <Composer
          errorMessage={
            conversation.submitError ? getCommandErrorMessage(conversation.submitError) : undefined
          }
          pending={conversation.isSubmitting}
          onSubmit={submitMessage}
        />
      </div>
    </section>
  )
}

const conversationStarterPromptKeys = ['starters.summarize', 'starters.next', 'starters.review']

function EmptyConversation({
  disabled,
  onPickExample,
  title,
}: {
  disabled: boolean
  onPickExample: (message: string) => void
  title: string
}) {
  const { t } = useTranslation('conversation')
  const conversationStarterPrompts = conversationStarterPromptKeys.map((promptKey) => t(promptKey))

  return (
    <section className="mx-auto flex min-h-full w-full max-w-xl flex-col items-center justify-center gap-7 py-16 text-center">
      <div className="flex flex-col items-center gap-3">
        <span className="grid size-11 place-items-center rounded-xl bg-accent-soft text-primary">
          <Sparkles className="size-5" />
        </span>
        <h1 className="font-semibold text-2xl tracking-tight">{title}</h1>
        <p className="max-w-md text-muted-foreground text-sm leading-relaxed">{t('intro')}</p>
      </div>
      <div className="flex w-full flex-col gap-2">
        {conversationStarterPrompts.map((prompt) => (
          <button
            className="rounded-lg border border-border bg-surface px-4 py-3 text-left text-foreground text-sm shadow-card transition-[background-color,box-shadow] hover:bg-muted disabled:pointer-events-none disabled:opacity-50"
            disabled={disabled}
            key={prompt}
            onClick={() => onPickExample(prompt)}
            type="button"
          >
            {prompt}
          </button>
        ))}
      </div>
    </section>
  )
}

function getConversationMessageElementId(messageId: string) {
  return `conversation-message-${messageId}`
}

function scrollToMessage(messageId: string) {
  const messageElement = document.getElementById(getConversationMessageElementId(messageId))

  if (!messageElement) {
    return
  }

  messageElement.scrollIntoView({ block: 'center', behavior: 'smooth' })
  messageElement.focus({ preventScroll: true })
}

function toRuntimeState(
  conversation: ConversationRecord,
  localMessages: OptimisticMessage[],
  events: RunEvent[],
  artifacts: Array<{
    actionLabel: string
    description: string
    id: string
    kind: string
    preview?: string
    sourceMessageId?: string
    sourceRunId: string
    status: 'failed' | 'pending' | 'ready' | 'running'
    title: string
  }>,
  labels: {
    continueLabel: string
    locale: string
    now: string
    reviewGeneratedFoundation: string
    userAuthor: string
  },
): ConversationRuntimeState {
  const messages = conversation.messages.map((message) => toMessageViewModel(message, labels))
  const allMessages = [...messages, ...localMessages.map((localMessage) => localMessage.message)]
  const artifactSourceMessageId = getLatestAssistantMessageId(messages)
  const messageIds = new Set(messages.map((message) => message.id))
  const renderedArtifacts = artifacts.map((artifact) => ({
    ...artifact,
    preview: artifact.preview ?? '',
    previewState: artifact.preview ? ('ready' as const) : ('loading' as const),
    sourceMessageId:
      artifact.sourceMessageId && messageIds.has(artifact.sourceMessageId)
        ? artifact.sourceMessageId
        : artifactSourceMessageId,
  }))

  return {
    activityItems: toActivityItems(events, labels.locale),
    artifacts: renderedArtifacts,
    decisions: [],
    diffPreview: toDiffPreview(renderedArtifacts),
    messages: allMessages,
    nextActions: [],
    planItems: toPlanItems(allMessages),
    reviewRequest:
      renderedArtifacts.length > 0
        ? {
            continueActionLabel: labels.continueLabel,
            continuePrompt: 'Continue',
            title: labels.reviewGeneratedFoundation,
          }
        : null,
  }
}

function createOptimisticMessage(
  conversation: ConversationRecord,
  body: string,
  labels: {
    now: string
    userAuthor: string
  },
): OptimisticMessage {
  optimisticMessageSequence += 1

  return {
    confirmed: false,
    knownUserMessageIds: new Set(
      conversation.messages
        .filter((message) => message.author === 'user')
        .map((message) => message.id),
    ),
    message: {
      author: labels.userAuthor,
      avatar: 'Y',
      body,
      id: `message-optimistic-${optimisticMessageSequence}`,
      time: labels.now,
      tone: 'user',
    },
  }
}

function hasNewPersistedUserMessage(
  conversation: ConversationRecord,
  localMessage: OptimisticMessage,
) {
  return conversation.messages.some(
    (message) =>
      message.author === 'user' &&
      message.body === localMessage.message.body &&
      !localMessage.knownUserMessageIds.has(message.id),
  )
}

function toMessageViewModel(
  message: ConversationRecord['messages'][number],
  labels: {
    locale: string
    userAuthor: string
  },
): ConversationRuntimeState['messages'][number] {
  const isAssistant = message.author === 'assistant'

  return {
    author: isAssistant ? 'Jyowo' : labels.userAuthor,
    avatar: isAssistant ? 'J' : 'Y',
    body: message.body,
    id: message.id,
    time: formatTimestamp(message.timestamp, labels.locale),
    tone: isAssistant ? 'assistant' : 'user',
  }
}

function getLatestAssistantMessageId(messages: ConversationRuntimeState['messages']) {
  for (let index = messages.length - 1; index >= 0; index -= 1) {
    const message = messages[index]

    if (message?.tone === 'assistant') {
      return message.id
    }
  }

  return undefined
}

function formatTimestamp(timestamp: string, locale: string) {
  const date = new Date(timestamp)

  if (Number.isNaN(date.getTime())) {
    return timestamp
  }

  return new Intl.DateTimeFormat(locale, {
    hour: 'numeric',
    minute: '2-digit',
  }).format(date)
}

function formatMultilineMessageBody(body: string) {
  const lines = body.split('\n')

  if (lines.length === 1) {
    return lines[0] ?? ''
  }

  return (
    <>
      {lines.map((line, index) => (
        <span key={`${index}:${line}`}>
          {line}
          {index < lines.length - 1 ? <br /> : null}
        </span>
      ))}
    </>
  )
}

function toMessageBodyNode(
  message: ConversationRuntimeState['messages'][number],
  options: { stripPlanBlock: boolean },
) {
  if (message.tone !== 'assistant') {
    return formatMultilineMessageBody(message.body)
  }

  const body = options.stripPlanBlock ? removeExplicitPlanBlock(message.body) : message.body

  if (!body.trim()) {
    return undefined
  }

  return <MarkdownMessage>{body}</MarkdownMessage>
}

function toPlanBlockItem(item: ConversationRuntimeState['planItems'][number]) {
  return {
    label: item.label,
    status: item.status === 'completed' ? 'Done' : 'In progress',
  } as const
}

function toPlanItems(
  messages: ConversationRuntimeState['messages'],
): ConversationRuntimeState['planItems'] {
  const assistantMessage = [...messages].reverse().find((message) => message.tone === 'assistant')

  if (!assistantMessage) {
    return []
  }

  return getExplicitPlanLines(assistantMessage.body)
    .map(parsePlanLine)
    .filter((item): item is ConversationRuntimeState['planItems'][number] => Boolean(item))
}

function getExplicitPlanLines(body: string) {
  const lines = body.split('\n')
  const planStartIndex = lines.findIndex((line) => /^\s*Plan:\s*$/i.test(line))

  if (planStartIndex < 0) {
    return []
  }

  const planLines: string[] = []

  for (const line of lines.slice(planStartIndex + 1)) {
    if (parsePlanLine(line)) {
      planLines.push(line)
      continue
    }

    if (line.trim().length === 0) {
      continue
    }

    break
  }

  return planLines
}

function removeExplicitPlanBlock(body: string) {
  const lines = body.split('\n')
  const planStartIndex = lines.findIndex((line) => /^\s*Plan:\s*$/i.test(line))

  if (planStartIndex < 0) {
    return body
  }

  const linesToRemove = new Set<number>([planStartIndex])

  for (let index = planStartIndex + 1; index < lines.length; index += 1) {
    const line = lines[index] ?? ''

    if (parsePlanLine(line)) {
      linesToRemove.add(index)
      continue
    }

    if (line.trim().length === 0) {
      linesToRemove.add(index)
      continue
    }

    break
  }

  return lines
    .filter((_line, index) => !linesToRemove.has(index))
    .join('\n')
    .trim()
}

function parsePlanLine(line: string): ConversationRuntimeState['planItems'][number] | undefined {
  const checklistMatch = /^\s*[-*]\s+\[(?<checked>[ xX])\]\s+(?<label>.+?)\s*$/.exec(line)

  if (checklistMatch?.groups) {
    const label = checklistMatch.groups.label?.trim()

    if (!label) {
      return undefined
    }

    return {
      id: `plan-${label}`,
      label,
      status: checklistMatch.groups.checked?.toLowerCase() === 'x' ? 'completed' : 'pending',
    }
  }

  const numberedMatch = /^\s*\d+[.)]\s+(?<label>.+?)\s*$/.exec(line)
  const label = numberedMatch?.groups?.label?.trim()

  if (!label) {
    return undefined
  }

  return {
    id: `plan-${label}`,
    label,
    status: 'pending',
  }
}

function toActivityItems(
  events: RunEvent[],
  locale: string,
): ConversationRuntimeState['activityItems'] {
  return events.slice(-3).map((event) => ({
    id: event.id,
    label: getActivityLabel(event),
    status: getActivityStatus(event.type),
    time: formatTimestamp(event.timestamp, locale),
  }))
}

function getActivityLabel(event: RunEvent) {
  if (event.visibility === 'withheld') {
    return getWithheldActivityLabel(event.type)
  }

  if (event.type === 'tool.requested') {
    return event.payload?.toolName ?? 'tool'
  }

  if (
    event.type === 'tool.approved' ||
    event.type === 'tool.denied' ||
    event.type === 'tool.completed' ||
    event.type === 'tool.failed'
  ) {
    return event.payload?.toolUseId ?? 'tool'
  }

  if (event.type === 'permission.requested' || event.type === 'permission.resolved') {
    return event.payload?.requestId ?? 'permission'
  }

  return getWithheldActivityLabel(event.type)
}

function getWithheldActivityLabel(type: RunEvent['type']) {
  if (type.startsWith('tool.')) {
    return 'tool'
  }

  if (type.startsWith('permission.')) {
    return 'permission'
  }

  if (type.startsWith('assistant.')) {
    return 'assistant'
  }

  return 'run'
}

function getActivityStatus(
  type: string,
): ConversationRuntimeState['activityItems'][number]['status'] {
  if (type === 'engine.failed' || type === 'tool.failed') {
    return 'failed'
  }

  if (type === 'permission.requested' || type === 'tool.denied') {
    return 'blocked'
  }

  if (type === 'tool.requested') {
    return 'queued'
  }

  if (type === 'run.started' || type === 'assistant.delta' || type === 'tool.approved') {
    return 'running'
  }

  return 'success'
}

function toDiffPreview(
  artifacts: ConversationRuntimeState['artifacts'],
): ConversationRuntimeState['diffPreview'] {
  const diffArtifact = artifacts.find(
    (artifact) => artifact.kind === 'diff' && artifact.preview.trim().length > 0,
  )

  if (!diffArtifact) {
    return null
  }

  const lines = diffArtifact.preview.split('\n').slice(0, 80)

  return {
    addedLineCount: lines.filter((line) => line.startsWith('+') && !line.startsWith('+++')).length,
    filename: diffArtifact.title,
    lines: lines.map((line, index) => ({
      content: line.replace(/^[+-]\s?/, ''),
      lineNumber: index + 1,
      type:
        line.startsWith('+') && !line.startsWith('+++')
          ? 'added'
          : line.startsWith('-') && !line.startsWith('---')
            ? 'removed'
            : 'context',
    })),
  }
}
