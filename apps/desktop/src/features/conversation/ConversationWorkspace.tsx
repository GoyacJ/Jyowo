import { useQuery } from '@tanstack/react-query'
import { useEffect, useState } from 'react'

import type { RunEvent } from '@/shared/events/run-event-schema'
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
  const commandClient = useCommandClient()
  const conversation = useConversation({ conversationId })
  const [activeArtifactId, setActiveArtifactId] = useState<string | null>(null)
  const [localMessages, setLocalMessages] = useState<OptimisticMessage[]>([])
  const activityQuery = useQuery({
    enabled: Boolean(conversation.selectedConversationId),
    queryFn: () =>
      commandClient.listActivity({
        conversationId: conversation.selectedConversationId ?? '',
        runId: undefined,
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
        <p className="pt-4 text-muted-foreground text-sm">Loading conversation</p>
      </section>
    )
  }

  if (conversation.error) {
    return (
      <section className="mx-auto flex min-h-full max-w-5xl flex-col">
        <h1 className="pt-4 font-semibold text-2xl tracking-normal">Conversation unavailable</h1>
        <p className="mt-3 text-destructive text-sm">
          {getCommandErrorMessage(conversation.error)}
        </p>
      </section>
    )
  }

  if (conversation.isEmpty || !conversation.conversation) {
    return (
      <section className="mx-auto flex min-h-full max-w-5xl flex-col">
        <h1 className="pt-4 font-semibold text-2xl tracking-normal">No conversations yet</h1>
        <p className="mt-3 text-muted-foreground text-sm">
          Start from the composer when a workspace conversation is available.
        </p>
      </section>
    )
  }

  const currentConversation = conversation.conversation
  const runtimeState = toRuntimeState(
    currentConversation,
    localMessages,
    activityQuery.data?.events ?? [],
    artifactsQuery.data?.artifacts ?? [],
  )
  const completedPlanCount = runtimeState.planItems.filter(
    (item) => item.status === 'completed',
  ).length
  const activeActivity = runtimeState.activityItems[0]
  const workMessageId = getLatestAssistantMessageId(runtimeState.messages)

  return (
    <section className="mx-auto grid h-full min-h-0 w-full max-w-[870px] grid-rows-[minmax(0,1fr)_auto]">
      <div className="min-h-0 overflow-auto pr-1">
        <ConversationCanvas title={currentConversation.title}>
          {runtimeState.messages.map((message) => (
            <ConversationMessage
              author={message.author}
              avatar={message.avatar}
              body={formatMultilineMessageBody(message.body)}
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
                      title={runtimeState.reviewRequest.title}
                    />
                  ) : null}
                </>
              ) : null}
            </ConversationMessage>
          ))}
        </ConversationCanvas>
      </div>

      <div className="pt-4">
        <Composer
          errorMessage={
            conversation.submitError ? getCommandErrorMessage(conversation.submitError) : undefined
          }
          pending={conversation.isSubmitting}
          onSubmit={(message) => {
            const optimisticMessage = createOptimisticMessage(currentConversation, message)
            setLocalMessages((currentMessages) => [...currentMessages, optimisticMessage])
            void conversation
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
          }}
        />
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
): ConversationRuntimeState {
  const messages = conversation.messages.map(toMessageViewModel)
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
    activityItems: toActivityItems(events),
    artifacts: renderedArtifacts,
    decisions: [],
    diffPreview: toDiffPreview(renderedArtifacts),
    messages: allMessages,
    nextActions: [],
    planItems: toPlanItems(allMessages),
    reviewRequest:
      renderedArtifacts.length > 0
        ? {
            continueActionLabel: 'Continue',
            title: 'Review generated foundation',
          }
        : null,
  }
}

function createOptimisticMessage(
  conversation: ConversationRecord,
  body: string,
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
      author: 'You',
      avatar: 'Y',
      body,
      id: `message-optimistic-${optimisticMessageSequence}`,
      time: 'Now',
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
): ConversationRuntimeState['messages'][number] {
  const isAssistant = message.author === 'assistant'

  return {
    author: isAssistant ? 'Jyowo' : 'You',
    avatar: isAssistant ? 'J' : 'Y',
    body: message.body,
    id: message.id,
    time: formatTimestamp(message.timestamp),
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

function formatTimestamp(timestamp: string) {
  const date = new Date(timestamp)

  if (Number.isNaN(date.getTime())) {
    return timestamp
  }

  return new Intl.DateTimeFormat('en-US', {
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

function toPlanBlockItem(item: ConversationRuntimeState['planItems'][number]) {
  return {
    label: item.label,
    status: item.status === 'completed' ? 'Done' : 'In progress',
  } as const
}

function toPlanItems(
  _messages: ConversationRuntimeState['messages'],
): ConversationRuntimeState['planItems'] {
  return []
}

function toActivityItems(events: RunEvent[]): ConversationRuntimeState['activityItems'] {
  return events.slice(-3).map((event) => ({
    id: event.id,
    label: getActivityLabel(event),
    status: getActivityStatus(event.type),
    time: formatTimestamp(event.timestamp),
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
