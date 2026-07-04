import { Check, Clock, Download, ExternalLink, Loader2, TriangleAlert } from 'lucide-react'
import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import { cn } from '@/shared/lib/utils'
import { useCommandClient } from '@/shared/tauri/react'
import { ArtifactPreview } from './ArtifactPreview'
import type { ArtifactRevisionSummary, ArtifactSegment } from '@/shared/tauri/commands'

export function ArtifactPane({
  artifact,
  conversationId,
  revisionId,
}: {
  artifact: ArtifactSegment
  conversationId: string
  revisionId?: string
}) {
  const { t } = useTranslation('artifacts')
  const revision = artifact.revision
  const media = revision.media
  const [activeTab, setActiveTab] = useState<'preview' | 'info'>('preview')
  const [fetchingContent, setFetchingContent] = useState(false)
  const [fetchedContent, setFetchedContent] = useState<string | null>(null)
  const commandClient = useCommandClient()

  const handleFetchContent = async () => {
    if (!revision.contentRef) return
    setFetchingContent(true)
    try {
      const response = await commandClient.getArtifactRevisionContent({
        conversationId,
        contentRef: revision.contentRef,
      })
      setFetchedContent(response.content)
    } catch {
      setFetchedContent(null)
    } finally {
      setFetchingContent(false)
    }
  }

  return (
    <div className="flex h-full flex-col">
      {/* Header */}
      <div className="border-border border-b px-4 py-3">
        <h3 className="font-medium text-sm">{artifact.title}</h3>
        <div className="mt-1.5 flex flex-wrap items-center gap-2">
          <RevisionStatusBadge status={revision.status} />
          <span className="text-muted-foreground text-xs">{revision.kind}</span>
          <span className="text-muted-foreground text-xs">
            {t('revisionId')}: {revision.revisionId}
          </span>
        </div>
        {revision.summary ? (
          <p className="mt-1 text-muted-foreground text-xs">{revision.summary}</p>
        ) : null}
      </div>

      {/* Tabs */}
      <div className="flex border-border border-b">
        <TabButton active={activeTab === 'preview'} onClick={() => setActiveTab('preview')}>
          {t('preview')}
        </TabButton>
        <TabButton active={activeTab === 'info'} onClick={() => setActiveTab('info')}>
          {t('info')}
        </TabButton>
      </div>

      {/* Content */}
      <div className="min-h-0 flex-1 overflow-auto">
        {activeTab === 'preview' ? (
          <div className="p-4">
            {revision.kind === 'image' && media ? (
              <img
                alt={artifact.title}
                className="max-h-80 w-full rounded-md border border-border object-contain"
                src={media.mimeType && revision.contentRef
                  ? `data:${media.mimeType};base64,` // placeholder
                  : undefined}
              />
            ) : revision.kind === 'html' ? (
              <iframe
                className="h-80 w-full rounded-md border border-border"
                sandbox="allow-scripts"
                srcDoc={fetchedContent ?? undefined}
                title={artifact.title}
              />
            ) : (
              <div className="space-y-3">
                {revision.contentRef && !fetchedContent ? (
                  <button
                    className="inline-flex items-center gap-2 rounded-md border border-border px-3 py-2 text-sm hover:bg-muted"
                    disabled={fetchingContent}
                    onClick={handleFetchContent}
                    type="button"
                  >
                    {fetchingContent ? (
                      <Loader2 className="size-4 animate-spin" />
                    ) : (
                      <Download className="size-4" />
                    )}
                    {fetchingContent ? t('fetching') : t('fetchContent')}
                  </button>
                ) : null}
                {fetchedContent ? (
                  <ArtifactPreview
                    content={fetchedContent}
                    kind={revision.kind}
                    state="ready"
                    title={artifact.title}
                  />
                ) : null}
                {!revision.contentRef && !fetchedContent ? (
                  <p className="text-muted-foreground text-sm">{t('noContentAvailable')}</p>
                ) : null}
              </div>
            )}
          </div>
        ) : (
          <div className="space-y-3 p-4">
            <InfoRow label={t('artifactId')} value={artifact.artifactId} />
            <InfoRow label={t('revisionId')} value={revision.revisionId} />
            <InfoRow label={t('kind')} value={revision.kind} />
            <InfoRow label={t('status')} value={revision.status} />
            <InfoRow label={t('sourceRunId')} value={revision.sourceRunId} />
            {media ? (
              <>
                <InfoRow label={t('mediaType')} value={media.mimeType} />
                <InfoRow label={t('mediaSize')} value={formatBytes(media.sizeBytes)} />
              </>
            ) : null}
            {revision.contentRef ? (
              <InfoRow label={t('contentRef')} value={revision.contentRef} />
            ) : null}
          </div>
        )}
      </div>
    </div>
  )
}

function TabButton({
  active,
  children,
  onClick,
}: {
  active: boolean
  children: React.ReactNode
  onClick: () => void
}) {
  return (
    <button
      className={cn(
        'border-border border-b-2 px-4 py-2 text-sm transition-colors',
        active
          ? 'border-primary text-foreground'
          : 'border-transparent text-muted-foreground hover:text-foreground',
      )}
      onClick={onClick}
      type="button"
    >
      {children}
    </button>
  )
}

function RevisionStatusBadge({ status }: { status: ArtifactRevisionSummary['status'] }) {
  const icons = {
    pending: Clock,
    running: Loader2,
    ready: Check,
    failed: TriangleAlert,
  }
  const colors = {
    pending: 'text-muted-foreground',
    running: 'text-primary',
    ready: 'text-success',
    failed: 'text-destructive',
  }
  const Icon = icons[status]
  return (
    <span className={cn('inline-flex items-center gap-1 font-medium text-xs', colors[status])}>
      <Icon className="size-3" />
      {status}
    </span>
  )
}

function InfoRow({ label, value }: { label: string; value: string }) {
  return (
    <div className="grid grid-cols-[120px_minmax(0,1fr)] gap-2 text-xs">
      <span className="text-muted-foreground">{label}</span>
      <span className="truncate font-mono">{value}</span>
    </div>
  )
}

function formatBytes(sizeBytes: number) {
  if (sizeBytes < 1024) return `${sizeBytes} B`
  if (sizeBytes < 1024 * 1024) return `${(sizeBytes / 1024).toFixed(1)} KB`
  return `${(sizeBytes / (1024 * 1024)).toFixed(1)} MB`
}
