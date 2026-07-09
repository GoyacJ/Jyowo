import { useMutation, useQueryClient } from '@tanstack/react-query'
import { useNavigate } from '@tanstack/react-router'
import { Sparkles } from 'lucide-react'
import { useTranslation } from 'react-i18next'
import { conversationQueryKeys } from '@/features/conversation/use-conversation'
import { ProjectSelectorActions } from '@/features/workspace/ProjectSelector'
import { onProjectWorkspaceChanged } from '@/features/workspace/reset-workspace-scope'
import { addProject, createConversation } from '@/shared/tauri/commands'
import { pickProjectDirectory } from '@/shared/tauri/file-dialog'
import { useCommandClient } from '@/shared/tauri/react'
import appIconUrl from '../../../src-tauri/icons/32x32.png'

type WelcomeWorkspaceProps = {
  onConversationCreated: (conversationId: string) => void
}

export function WelcomeWorkspace({ onConversationCreated }: WelcomeWorkspaceProps) {
  const { t } = useTranslation(['shell', 'conversation'])
  const commandClient = useCommandClient()
  const queryClient = useQueryClient()
  const navigate = useNavigate()
  const addProjectMutation = useMutation({
    mutationFn: (path: string) => addProject(path, commandClient),
    onSuccess: async () => {
      await onProjectWorkspaceChanged(queryClient, navigate)
    },
  })
  const createConversationMutation = useMutation({
    mutationFn: () => createConversation(commandClient),
    onSuccess: async (response) => {
      await queryClient.invalidateQueries({ queryKey: conversationQueryKeys.all })
      onConversationCreated(response.conversation.id)
    },
  })

  async function openProject() {
    const selectedPath = await pickProjectDirectory()
    if (!selectedPath) {
      return
    }

    addProjectMutation.mutate(selectedPath)
  }

  function createNewConversation() {
    createConversationMutation.mutate()
  }

  return (
    <section className="mx-auto flex min-h-full max-w-3xl flex-col items-center justify-center px-4 py-16 text-center">
      <div className="grid size-16 place-items-center rounded-md border border-border bg-surface shadow-sm transition-[border-color,box-shadow] duration-300 hover:border-primary/45 hover:shadow-md animate-page-enter [animation-delay:60ms]">
        <img alt="" className="size-10" height={40} src={appIconUrl} width={40} />
      </div>
      <div className="mt-6 flex items-center gap-2 text-muted-foreground text-sm animate-page-enter [animation-delay:150ms]">
        <Sparkles aria-hidden="true" className="size-4 text-primary" />
        <span>{t('shell:welcome.eyebrow')}</span>
      </div>
      <h1 className="mt-3 font-semibold text-3xl tracking-normal animate-page-enter [animation-delay:240ms]">
        {t('shell:welcome.title')}
      </h1>
      <p className="mt-3 max-w-xl text-muted-foreground text-sm leading-6 animate-page-enter [animation-delay:330ms]">
        {t('shell:welcome.description')}
      </p>
      <div className="mt-8 animate-page-enter [animation-delay:420ms]">
        <ProjectSelectorActions
          onNewConversation={createNewConversation}
          onOpenProject={() => void openProject()}
          showNewConversation
        />
      </div>
    </section>
  )
}
