import { useQuery, useQueryClient } from '@tanstack/react-query'
import { useEffect, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'

import type { PermissionMode } from '@/shared/tauri/commands'
import { getCommandErrorMessage } from '@/shared/tauri/errors'
import { pickAttachmentPath } from '@/shared/tauri/file-dialog'
import { useCommandClient } from '@/shared/tauri/react'
import { Composer, type ComposerSubmitPayload } from './Composer'
import { ConversationTimeline } from './timeline/conversation-timeline'
import { useConversationTimeline } from './timeline/use-conversation-timeline'
import { conversationQueryKeys } from './use-conversation'

const defaultConversationTitle = 'New conversation'

type ConversationWorkspaceProps = {
  conversationId?: string
}

export function ConversationWorkspace({ conversationId }: ConversationWorkspaceProps) {
  const { t } = useTranslation(['conversation', 'shell'])
  const commandClient = useCommandClient()
  const queryClient = useQueryClient()
  const timeline = useConversationTimeline({ conversationId })
  const workspaceKey = timeline.workspacePath ?? 'none'
  const [composerPermissionMode, setComposerPermissionMode] = useState<PermissionMode>('default')
  const composerPermissionModeDirtyRef = useRef(false)
  const [selectedModelConfigId, setSelectedModelConfigId] = useState('')
  const selectedModelConfigDirtyRef = useRef(false)
  const providerSettingsQuery = useQuery({
    queryFn: () => commandClient.listProviderSettings(),
    queryKey: ['conversation-model-configs'],
  })
  const executionSettingsQuery = useQuery({
    enabled: Boolean(timeline.workspacePath),
    queryFn: () => {
      if (!timeline.workspacePath) {
        throw new Error('Workspace path is required')
      }
      return commandClient.getExecutionSettings({ workspacePath: timeline.workspacePath })
    },
    queryKey: ['conversation-execution-settings', workspaceKey],
  })
  useEffect(() => {
    composerPermissionModeDirtyRef.current = false
    setComposerPermissionMode('default')
    selectedModelConfigDirtyRef.current = false
  }, [workspaceKey])

  useEffect(() => {
    if (!executionSettingsQuery.data || composerPermissionModeDirtyRef.current) {
      return
    }

    setComposerPermissionMode(executionSettingsQuery.data.permissionMode)
  }, [executionSettingsQuery.data, workspaceKey])

  useEffect(() => {
    if (selectedModelConfigDirtyRef.current) {
      return
    }

    setSelectedModelConfigId(
      timeline.conversation?.modelConfigId ?? providerSettingsQuery.data?.defaultConfigId ?? '',
    )
  }, [
    providerSettingsQuery.data?.defaultConfigId,
    timeline.conversation?.id,
    timeline.conversation?.modelConfigId,
  ])

  if (timeline.isLoading) {
    return (
      <section className="mx-auto flex min-h-full max-w-5xl flex-col">
        <p className="pt-4 text-muted-foreground text-sm">Loading conversation...</p>
      </section>
    )
  }

  if (timeline.error) {
    return (
      <section className="mx-auto flex min-h-full max-w-5xl flex-col">
        <h1 className="pt-4 font-semibold text-2xl tracking-normal">Conversation unavailable</h1>
        <p className="mt-3 text-destructive text-sm">{getCommandErrorMessage(timeline.error)}</p>
      </section>
    )
  }

  if (timeline.isEmpty || !timeline.conversation) {
    return (
      <section className="mx-auto flex min-h-full max-w-5xl flex-col">
        <h1 className="pt-4 font-semibold text-2xl tracking-normal">Conversation unavailable</h1>
        <p className="mt-3 text-muted-foreground text-sm">
          This conversation is no longer available in the current project.
        </p>
      </section>
    )
  }

  const activeConversation = timeline.conversation
  const conversationTitle =
    activeConversation.title === defaultConversationTitle
      ? t('shell:conversations.defaultTitle')
      : activeConversation.title
  const configuredModelProfiles =
    providerSettingsQuery.data?.configs.filter((profile) => profile.hasApiKey) ?? []
  const currentModelProfile =
    configuredModelProfiles.find((profile) => profile.id === selectedModelConfigId) ?? null
  const modelConfigs = configuredModelProfiles.map((profile) => ({
    id: profile.id,
    label: `${profile.displayName} / ${profile.modelId}${
      profile.id === providerSettingsQuery.data?.defaultConfigId ? ' (default)' : ''
    }`,
  }))
  const executionSettings = executionSettingsQuery.data
  const composerPermissionModeReady =
    Boolean(executionSettings) &&
    (composerPermissionModeDirtyRef.current ||
      composerPermissionMode === executionSettings?.permissionMode)
  const composerDisabled =
    timeline.composerMode.kind === 'running-disabled' ||
    !composerPermissionModeReady ||
    !currentModelProfile

  function submitReviewContinue(prompt: string) {
    if (!composerPermissionModeReady) {
      return
    }

    void timeline.submitPrompt(emptySubmit(prompt, composerPermissionMode, selectedModelConfigId))
  }

  async function submitMessage(draft: ComposerSubmitPayload) {
    const response = await timeline.submitPrompt(draft)
    await queryClient.invalidateQueries({
      queryKey: conversationQueryKeys.detail(workspaceKey, activeConversation.id),
    })
    await queryClient.invalidateQueries({ queryKey: conversationQueryKeys.list(workspaceKey) })
    return response
  }

  return (
    <section className="mx-auto grid h-full min-h-0 w-full max-w-[900px] grid-rows-[minmax(0,1fr)_auto]">
      <ConversationTimeline
        turns={timeline.turns}
        title={conversationTitle}
        onPermissionResolve={(request) => {
          void timeline.resolvePermission(request)
        }}
        onReviewContinue={submitReviewContinue}
      />
      <div className="pt-4">
        <Composer
          disabled={composerDisabled}
          errorMessage={
            timeline.cancelError
              ? getCommandErrorMessage(timeline.cancelError)
              : timeline.submitError
                ? getCommandErrorMessage(timeline.submitError)
                : executionSettingsQuery.error
                  ? getCommandErrorMessage(executionSettingsQuery.error)
                  : undefined
          }
          cancelPending={timeline.isCancelling}
          modelCapability={currentModelProfile?.modelDescriptor?.conversationCapability ?? null}
          modelConfigDisabled={
            timeline.isSubmitting || timeline.composerMode.kind === 'running-disabled'
          }
          modelConfigId={selectedModelConfigId}
          modelConfigs={modelConfigs}
          mode={timeline.composerMode}
          onCreateAttachmentFromPath={commandClient.createAttachmentFromPath}
          onCancelRun={timeline.cancelActiveRun}
          onListReferenceCandidates={() =>
            commandClient.listReferenceCandidates({ conversationId: activeConversation.id })
          }
          onModelConfigChange={(modelConfigId) => {
            selectedModelConfigDirtyRef.current = true
            setSelectedModelConfigId(modelConfigId)
          }}
          permissionMode={composerPermissionMode}
          autoModeAvailable={executionSettings?.autoModeAvailable ?? false}
          onPermissionModeChange={(nextMode) => {
            composerPermissionModeDirtyRef.current = true
            setComposerPermissionMode(nextMode)
          }}
          onPickAttachmentPath={pickAttachmentPath}
          onSubmit={submitMessage}
        />
      </div>
    </section>
  )
}

function emptySubmit(
  prompt: string,
  permissionMode: PermissionMode,
  modelConfigId: string,
): ComposerSubmitPayload {
  return {
    attachments: [],
    contextReferences: [],
    modelConfigId,
    permissionMode,
    prompt,
  }
}
