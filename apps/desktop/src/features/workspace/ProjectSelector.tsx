import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { useNavigate } from '@tanstack/react-router'
import { Check, ChevronDown, FolderOpen, Plus } from 'lucide-react'
import { useTranslation } from 'react-i18next'

import { cn } from '@/shared/lib/utils'
import { addProject, listProjects, switchProject } from '@/shared/tauri/commands'
import { getCommandErrorMessage } from '@/shared/tauri/errors'
import { pickProjectDirectory } from '@/shared/tauri/file-dialog'
import { useCommandClient } from '@/shared/tauri/react'
import { Button } from '@/shared/ui/button'
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from '@/shared/ui/dropdown-menu'
import appIconUrl from '../../../src-tauri/icons/32x32.png'
import { onProjectWorkspaceChanged } from './reset-workspace-scope'

type ProjectSelectorProps = {
  compact?: boolean
}

export function ProjectSelector({ compact = false }: ProjectSelectorProps) {
  const { t } = useTranslation('shell')
  const commandClient = useCommandClient()
  const queryClient = useQueryClient()
  const navigate = useNavigate()
  const projectsQuery = useQuery({
    queryKey: ['projects', 'list'],
    queryFn: () => listProjects(commandClient),
  })

  const switchMutation = useMutation({
    mutationFn: (path: string) => switchProject(path, commandClient),
    onSuccess: async () => {
      await onProjectWorkspaceChanged(queryClient, navigate)
    },
  })

  const addMutation = useMutation({
    mutationFn: (path: string) => addProject(path, commandClient),
    onSuccess: async () => {
      await onProjectWorkspaceChanged(queryClient, navigate)
    },
  })

  const activePath = projectsQuery.data?.activePath ?? null
  const activeProject =
    projectsQuery.data?.projects.find((project) => project.path === activePath) ?? null
  const errorMessage =
    switchMutation.error || addMutation.error
      ? getCommandErrorMessage(switchMutation.error ?? addMutation.error)
      : projectsQuery.error
        ? getCommandErrorMessage(projectsQuery.error)
        : undefined
  const pending = switchMutation.isPending || addMutation.isPending
  const projectTitle = activeProject?.name ?? t('projects.noneSelected')

  async function openProjectDirectory() {
    const selectedPath = await pickProjectDirectory()
    if (!selectedPath) {
      return
    }

    addMutation.mutate(selectedPath)
  }

  function selectProject(path: string) {
    if (path === activePath) {
      return
    }

    switchMutation.mutate(path)
  }

  const projectMenu = (
    <DropdownMenu>
      <DropdownMenuTrigger asChild>
        {compact ? (
          <button
            aria-label={t('projects.switch')}
            className="grid size-9 place-items-center rounded-md text-muted-foreground hover:bg-muted hover:text-foreground"
            disabled={pending || projectsQuery.isLoading}
            title={projectTitle}
            type="button"
          >
            <FolderOpen className="size-4" />
          </button>
        ) : (
          <button
            aria-label={t('projects.switch')}
            className="flex min-w-0 flex-1 items-center gap-1.5 rounded-md py-1 pr-1 pl-0.5 text-left text-muted-foreground hover:bg-muted hover:text-foreground"
            disabled={pending || projectsQuery.isLoading}
            type="button"
          >
            <span className="min-w-0 flex-1">
              <span className="block truncate font-medium text-foreground text-sm">
                {projectTitle}
              </span>
              <span className="block truncate text-muted-foreground text-xs">
                {activeProject?.path ?? t('projects.pickDirectoryHint')}
              </span>
            </span>
            <ChevronDown aria-hidden="true" className="size-4 shrink-0 opacity-70" />
          </button>
        )}
      </DropdownMenuTrigger>
      <DropdownMenuContent align="start" className="w-[min(calc(100vw-6rem),18rem)]">
        {projectsQuery.data?.projects.map((project) => (
          <DropdownMenuItem key={project.path} onSelect={() => selectProject(project.path)}>
            <FolderOpen aria-hidden="true" className="size-4 text-muted-foreground" />
            <span className="min-w-0 flex-1 truncate">{project.name}</span>
            {project.path === activePath ? (
              <Check aria-hidden="true" className="size-4 text-foreground" />
            ) : null}
          </DropdownMenuItem>
        ))}
        {projectsQuery.data?.projects.length ? <DropdownMenuSeparator /> : null}
        <DropdownMenuItem onSelect={() => void openProjectDirectory()}>
          <Plus aria-hidden="true" className="size-4" />
          {t('projects.new')}
        </DropdownMenuItem>
      </DropdownMenuContent>
    </DropdownMenu>
  )

  if (compact) {
    return (
      <div className="flex w-full flex-col items-center gap-2">
        <span className="grid size-9 place-items-center" title={t('workspace')}>
          <img alt="" className="size-6" src={appIconUrl} />
        </span>
        {projectMenu}
        {errorMessage ? (
          <p className={cn('max-w-full truncate px-1 text-center text-destructive text-xs')}>
            {errorMessage}
          </p>
        ) : null}
      </div>
    )
  }

  return (
    <div className="min-w-0 flex-1">
      <div className="flex min-w-0 items-center gap-2">
        <span className="grid size-8 shrink-0 place-items-center">
          <img alt="" className="size-6" src={appIconUrl} />
        </span>
        {projectMenu}
      </div>
      {errorMessage ? (
        <p className={cn('mt-1 truncate px-0.5 text-destructive text-xs')}>{errorMessage}</p>
      ) : null}
    </div>
  )
}

export function ProjectSelectorActions({
  onOpenProject,
  onNewConversation,
  showNewConversation,
}: {
  onOpenProject: () => void
  onNewConversation: () => void
  showNewConversation: boolean
}) {
  const { t } = useTranslation('shell')

  return (
    <div className="flex flex-wrap gap-3">
      <Button onClick={onOpenProject} type="button" variant="default">
        <FolderOpen className="size-4" />
        {t('projects.open')}
      </Button>
      {showNewConversation ? (
        <Button onClick={onNewConversation} type="button" variant="outline">
          <Plus className="size-4" />
          {t('actions.newConversation')}
        </Button>
      ) : null}
    </div>
  )
}
