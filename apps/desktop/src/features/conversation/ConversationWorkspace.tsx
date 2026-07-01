import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
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
  const setModelConfigMutation = useMutation({
    mutationFn: (modelConfigId: string) => {
      if (!timeline.conversation) {
        throw new Error('No conversation selected')
      }

      return commandClient.setConversationModelConfig(timeline.conversation.id, modelConfigId)
    },
    onSuccess: async () => {
      if (timeline.conversation) {
        await queryClient.invalidateQueries({
          queryKey: conversationQueryKeys.detail(workspaceKey, timeline.conversation.id),
        })
      }
      await queryClient.invalidateQueries({ queryKey: ['conversation-model-configs'] })
    },
  })

  useEffect(() => {
    composerPermissionModeDirtyRef.current = false
    setComposerPermissionMode('default')
  }, [workspaceKey])

  useEffect(() => {
    if (!executionSettingsQuery.data || composerPermissionModeDirtyRef.current) {
      return
    }

    setComposerPermissionMode(executionSettingsQuery.data.permissionMode)
  }, [executionSettingsQuery.data, workspaceKey])

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
  const currentModelConfigId =
    activeConversation.modelConfigId ?? providerSettingsQuery.data?.defaultConfigId ?? ''
  const currentModelProfile =
    providerSettingsQuery.data?.configs.find((profile) => profile.id === currentModelConfigId) ??
    null
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
    timeline.composerMode.kind === 'running-disabled' || !composerPermissionModeReady

  function submitReviewContinue(prompt: string) {
    if (!composerPermissionModeReady) {
      return
    }

    void timeline.submitPrompt(emptySubmit(prompt, composerPermissionMode))
  }

  function submitMessage(draft: ComposerSubmitPayload) {
    return timeline.submitPrompt(draft)
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
          agentCapabilities={executionSettings?.agentCapabilities ?? null}
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
            timeline.isSubmitting || composerDisabled || setModelConfigMutation.isPending
          }
          modelConfigId={currentModelConfigId}
          modelConfigs={modelConfigs}
          mode={timeline.composerMode}
          onCreateAttachmentFromPath={commandClient.createAttachmentFromPath}
          onCancelRun={timeline.cancelActiveRun}
          onListReferenceCandidates={() =>
            commandClient.listReferenceCandidates({ conversationId: activeConversation.id })
          }
          onModelConfigChange={(modelConfigId) => setModelConfigMutation.mutate(modelConfigId)}
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

function emptySubmit(prompt: string, permissionMode: PermissionMode): ComposerSubmitPayload {
  return {
    attachments: [],
    contextReferences: [],
    permissionMode,
    prompt,
  }
}
