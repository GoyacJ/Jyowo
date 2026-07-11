import { FolderOpen, Plus } from 'lucide-react'
import { useTranslation } from 'react-i18next'

import { Button } from '@/shared/ui/button'

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
