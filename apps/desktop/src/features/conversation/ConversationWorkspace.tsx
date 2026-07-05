import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { PanelRightOpen } from 'lucide-react'
import { useEffect, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'

import { ContextPanel } from '@/features/context/ContextPanel'
import { useContextSnapshot } from '@/features/context/use-context-snapshot'
import { WorkbenchInspector } from '@/features/workbench/WorkbenchInspector'
import { useUiStore } from '@/shared/state/ui-store'
import type { MemoryThreadMode, PermissionMode } from '@/shared/tauri/commands'
import { getCommandErrorMessage } from '@/shared/tauri/errors'
import { pickAttachmentPath } from '@/shared/tauri/file-dialog'
import { useCommandClient } from '@/shared/tauri/react'
import { Button } from '@/shared/ui/button'
import { Composer, type ComposerSubmitPayload } from './Composer'
import { ConversationCanvas } from './ConversationCanvas'
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
  const [modelConfigOverridesByConversation, setModelConfigOverridesByConversation] = useState<
    Record<string, string>
  >({})
  const activeRunsByConversation = useUiStore((state) => state.activeRunsByConversation)
  const contextPanelCollapsed = useUiStore((state) => state.contextPanelCollapsed)
  const inspectorOpen = useUiStore((state) => state.inspectorOpen)
  const workbenchSelection = useUiStore((state) => state.workbenchSelection)
  const setContextPanelCollapsed = useUiStore((state) => state.setContextPanelCollapsed)
  const setInspectorOpen = useUiStore((state) => state.setInspectorOpen)
  const requestTimelineScroll = useUiStore((state) => state.requestTimelineScroll)
  const activeConversationId = timeline.conversation?.id
  const activeRunId = activeConversationId
    ? activeRunsByConversation[activeConversationId]
    : undefined
  const selectionConversationId =
    workbenchSelection && workbenchSelection.kind !== 'context'
      ? workbenchSelection.conversationId
      : undefined
  const inspectorSelectionMatches =
    Boolean(activeConversationId) &&
    Boolean(workbenchSelection) &&
    workbenchSelection?.kind !== 'context' &&
    selectionConversationId === activeConversationId
  const showInspector = inspectorOpen && inspectorSelectionMatches
  const showContext = Boolean(activeConversationId) && !showInspector && !contextPanelCollapsed
  const contextSnapshot = useContextSnapshot(
    activeConversationId
      ? activeRunId
        ? { conversationId: activeConversationId, runId: activeRunId }
        : { conversationId: activeConversationId }
      : {},
    { enabled: showContext },
  )
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
  const threadMemorySettingsQuery = useQuery({
    enabled: Boolean(timeline.conversation?.id),
    queryFn: () =>
      commandClient.getThreadMemorySettings({
        sessionId: timeline.conversation?.id ?? '',
      }),
    queryKey: ['conversation-thread-memory-settings', timeline.conversation?.id ?? 'none'],
  })
  const updateThreadMemorySettingsMutation = useMutation({
    mutationFn: (memoryMode: MemoryThreadMode) =>
      commandClient.updateThreadMemorySettings({
        settings: {
          generate_memories:
            threadMemorySettingsQuery.data?.settings.generate_memories ?? undefined,
          memory_mode: memoryMode,
          session_id: timeline.conversation?.id ?? '',
          use_memories: threadMemorySettingsQuery.data?.settings.use_memories ?? undefined,
        },
      }),
    onSuccess: (response) => {
      queryClient.setQueryData(
        ['conversation-thread-memory-settings', response.settings.session_id],
        response,
      )
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

  useEffect(() => {
    if (
      activeConversationId &&
      inspectorOpen &&
      workbenchSelection &&
      workbenchSelection.kind !== 'context' &&
      workbenchSelection.conversationId !== activeConversationId
    ) {
      setInspectorOpen(false)
    }
  }, [activeConversationId, inspectorOpen, setInspectorOpen, workbenchSelection])

  if (timeline.isLoading) {
    return (
      <section className="mx-auto flex min-h-full max-w-5xl flex-col">
        <p className="pt-4 text-muted-foreground text-sm">{t('conversation:loading')}</p>
      </section>
    )
  }

  if (timeline.error) {
    return (
      <section className="mx-auto flex min-h-full max-w-5xl flex-col">
        <h1 className="pt-4 font-semibold text-2xl tracking-normal">
          {t('conversation:unavailableTitle')}
        </h1>
        <p className="mt-3 text-destructive text-sm">{getCommandErrorMessage(timeline.error)}</p>
      </section>
    )
  }

  if (timeline.isEmpty || !timeline.conversation) {
    return (
      <section className="mx-auto flex min-h-full max-w-5xl flex-col">
        <h1 className="pt-4 font-semibold text-2xl tracking-normal">
          {t('conversation:unavailableTitle')}
        </h1>
        <p className="mt-3 text-muted-foreground text-sm">
          {t('conversation:unavailableDescription')}
        </p>
      </section>
    )
  }

  const activeConversation = timeline.conversation
  const renderedConversationId = activeConversation.id
  const contextPane = showContext ? (
    <ContextPanel
      context={contextSnapshot.context}
      errorMessage={
        contextSnapshot.error ? getCommandErrorMessage(contextSnapshot.error) : undefined
      }
      loading={contextSnapshot.isLoading}
      onClose={() => {
        setContextPanelCollapsed(true)
      }}
      onDecisionSelect={(decision) => {
        if (decision.requestId) {
          requestTimelineScroll(`permission:${decision.requestId}`)
        }
      }}
    />
  ) : null
  const inspectorPane = showInspector ? <WorkbenchInspector /> : null
  const conversationTitle =
    activeConversation.title === defaultConversationTitle
      ? t('shell:conversations.defaultTitle')
      : activeConversation.title
  const configuredModelProfiles =
    providerSettingsQuery.data?.configs.filter((profile) => profile.hasApiKey) ?? []
  const selectedModelConfigId =
    modelConfigOverridesByConversation[renderedConversationId] ??
    activeConversation.modelConfigId ??
    providerSettingsQuery.data?.defaultConfigId ??
    ''
  const currentModelProfile =
    configuredModelProfiles.find((profile) => profile.id === selectedModelConfigId) ?? null
  const modelConfigs = configuredModelProfiles.map((profile) => ({
    id: profile.id,
    label: `${profile.displayName} / ${profile.modelId}${
      profile.id === providerSettingsQuery.data?.defaultConfigId ? ' (default)' : ''
    }`,
  }))
  const executionSettings = executionSettingsQuery.data
  const threadMemorySettings = threadMemorySettingsQuery.data?.settings
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
    <ConversationCanvas
      actions={
        contextPanelCollapsed && !showInspector ? (
          <Button
            aria-label={t('shell:actions.showContextPanel')}
            className="size-8"
            onClick={() => {
              setInspectorOpen(false)
              setContextPanelCollapsed(false)
            }}
            size="icon"
            title={t('shell:actions.showContextPanel')}
            type="button"
            variant="outline"
          >
            <PanelRightOpen aria-hidden="true" className="size-4" />
          </Button>
        ) : null
      }
      rightPanel={inspectorPane ?? contextPane}
      rightPanelWidth={showInspector ? 360 : 320}
      title={conversationTitle}
    >
      <div className="grid h-full min-h-0 w-full grid-rows-[minmax(0,1fr)_auto]">
        <ConversationTimeline
          gapMarkers={timeline.gapMarkers}
          hasMoreAfter={timeline.hasMoreAfter}
          hasMoreBefore={timeline.hasMoreBefore}
          loadEarlier={timeline.loadEarlier}
          loadLater={timeline.loadLater}
          loadingEarlier={timeline.loadingEarlier}
          loadingLater={timeline.loadingLater}
          turns={timeline.turns}
          title={conversationTitle}
          showTitle={false}
          onPermissionResolve={(request) => {
            void timeline.resolvePermission(request)
          }}
          onReviewContinue={submitReviewContinue}
          retryGap={timeline.retryGap}
        />
        <div className="pt-4">
          <Composer
            key={renderedConversationId}
            conversationId={renderedConversationId}
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
              setModelConfigOverridesByConversation((current) => ({
                ...current,
                [renderedConversationId]: modelConfigId,
              }))
            }}
            permissionMode={composerPermissionMode}
            autoModeAvailable={executionSettings?.autoModeAvailable ?? false}
            onPermissionModeChange={(nextMode) => {
              composerPermissionModeDirtyRef.current = true
              setComposerPermissionMode(nextMode)
            }}
            memoryMode={threadMemorySettings?.memory_mode}
            memoryModeDisabled={
              threadMemorySettingsQuery.isLoading || updateThreadMemorySettingsMutation.isPending
            }
            onMemoryModeChange={(nextMode) => {
              updateThreadMemorySettingsMutation.mutate(nextMode)
            }}
            onPickAttachmentPath={pickAttachmentPath}
            onSubmit={submitMessage}
          />
        </div>
      </div>
    </ConversationCanvas>
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
