import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { Check, GitMerge, X } from 'lucide-react'
import { useState } from 'react'
import { useTranslation } from 'react-i18next'

import { useDaemonClient } from '@/shared/tauri/react'
import { Button } from '@/shared/ui/button'
import { Card, CardContent, CardHeader, CardTitle } from '@/shared/ui/card'
import { Checkbox } from '@/shared/ui/checkbox'
import { EmptyState } from '@/shared/ui/empty-state'
import { Section } from '@/shared/ui/section'

import { DEFAULT_MEMORY_TENANT_ID } from './memory-types'

const inboxQueryKeys = {
  all: (workspaceRoot: string | undefined) => ['memory-inbox', workspaceRoot ?? null] as const,
}

export function MemoryInbox({ workspaceRoot }: { workspaceRoot?: string }) {
  const { t } = useTranslation('memory')
  const daemonClient = useDaemonClient()
  const queryClient = useQueryClient()
  const [message, setMessage] = useState<string | null>(null)
  const [selectedCandidateIds, setSelectedCandidateIds] = useState<string[]>([])

  const inboxQuery = useQuery({
    queryKey: inboxQueryKeys.all(workspaceRoot),
    queryFn: () =>
      daemonClient.listMemoryCandidates(workspaceRoot, {
        limit: 50,
        tenant_id: DEFAULT_MEMORY_TENANT_ID,
      }),
  })

  const approveMutation = useMutation({
    mutationFn: (candidateId: string) =>
      daemonClient.approveMemoryCandidate(workspaceRoot, {
        candidate_id: candidateId,
        tenant_id: DEFAULT_MEMORY_TENANT_ID,
      }),
    onSuccess: () => {
      setMessage(t('candidateApproved'))
      queryClient.invalidateQueries({ queryKey: inboxQueryKeys.all(workspaceRoot) })
    },
  })

  const rejectMutation = useMutation({
    mutationFn: (candidateId: string) =>
      daemonClient.rejectMemoryCandidate(
        workspaceRoot,
        {
          candidate_id: candidateId,
          reason: 'rejected by user',
          tenant_id: DEFAULT_MEMORY_TENANT_ID,
        },
      ),
    onSuccess: () => {
      setMessage(t('candidateRejected'))
      queryClient.invalidateQueries({ queryKey: inboxQueryKeys.all(workspaceRoot) })
    },
  })

  const mergeMutation = useMutation({
    mutationFn: () => {
      const selectedCandidates = candidates.filter((candidate) =>
        selectedCandidateIds.includes(candidate.id),
      )
      const firstCandidate = selectedCandidates[0]
      if (!firstCandidate || selectedCandidates.length < 2) {
        throw new Error('Select at least two candidates.')
      }
      return daemonClient.mergeMemoryCandidate(
        workspaceRoot,
        {
          candidate_ids: selectedCandidates.map((candidate) => candidate.id),
          evidence: firstCandidate.evidence,
          merged_record: {
            content: selectedCandidates
              .map((candidate) => candidate.proposed_record.content.trim())
              .filter(Boolean)
              .join('\n\n'),
            expires_at: firstCandidate.proposed_record.expires_at ?? undefined,
            kind: firstCandidate.proposed_record.kind,
            metadata: {
              source_trust: firstCandidate.proposed_record.metadata.source_trust,
              tags: Array.from(
                new Set(
                  selectedCandidates.flatMap(
                    (candidate) => candidate.proposed_record.metadata.tags ?? [],
                  ),
                ),
              ),
              ttl: firstCandidate.proposed_record.metadata.ttl ?? null,
            },
            visibility: firstCandidate.proposed_record.visibility,
          },
          tenant_id: DEFAULT_MEMORY_TENANT_ID,
        },
      )
    },
    onSuccess: () => {
      setMessage(t('candidateMerged'))
      setSelectedCandidateIds([])
      queryClient.invalidateQueries({ queryKey: inboxQueryKeys.all(workspaceRoot) })
    },
  })

  if (inboxQuery.isLoading) {
    return <div className="text-muted-foreground text-sm">{t('loading')}</div>
  }
  if (inboxQuery.isError) {
    return <div className="text-destructive text-sm">{t('errorLoading')}</div>
  }

  const candidates = inboxQuery.data?.candidates ?? []
  const selectedCount = selectedCandidateIds.length

  if (candidates.length === 0) {
    return <EmptyState>{t('inboxEmpty')}</EmptyState>
  }

  return (
    <Section>
      {message && <div className="rounded bg-success/10 p-2 text-sm text-success">{message}</div>}
      <div className="flex items-center justify-between gap-3">
        <p className="text-muted-foreground text-sm">
          {t('selectedCandidates', { count: selectedCount })}
        </p>
        <Button
          size="sm"
          variant="outline"
          onClick={() => mergeMutation.mutate()}
          disabled={selectedCount < 2 || mergeMutation.isPending}
        >
          <GitMerge className="mr-1 h-3 w-3" />
          {t('merge')}
        </Button>
      </div>
      {candidates.map((candidate) => (
        <Card key={candidate.id}>
          <CardHeader>
            <div className="flex items-center gap-3">
              <Checkbox
                aria-label={t('selectCandidate')}
                checked={selectedCandidateIds.includes(candidate.id)}
                disabled={candidate.state !== 'proposed'}
                onCheckedChange={(checked) => {
                  setSelectedCandidateIds((currentIds) =>
                    checked === true
                      ? [...currentIds, candidate.id]
                      : currentIds.filter((id) => id !== candidate.id),
                  )
                }}
              />
              <CardTitle className="text-sm">
                {formatMemoryKind(candidate.proposed_record.kind)} · {candidate.state}
              </CardTitle>
            </div>
          </CardHeader>
          <CardContent>
            <p className="text-sm text-muted-foreground line-clamp-3">
              {candidate.proposed_record.content}
            </p>
            <div className="mt-3 flex gap-2">
              {candidate.state === 'proposed' && (
                <>
                  <Button
                    size="sm"
                    variant="outline"
                    onClick={() => approveMutation.mutate(candidate.id)}
                    disabled={approveMutation.isPending}
                  >
                    <Check className="mr-1 h-3 w-3" />
                    {t('approve')}
                  </Button>
                  <Button
                    size="sm"
                    variant="outline"
                    onClick={() => rejectMutation.mutate(candidate.id)}
                    disabled={rejectMutation.isPending}
                  >
                    <X className="mr-1 h-3 w-3" />
                    {t('reject')}
                  </Button>
                </>
              )}
            </div>
          </CardContent>
        </Card>
      ))}
    </Section>
  )
}

function formatMemoryKind(kind: string | { custom: string }) {
  return typeof kind === 'string' ? kind : kind.custom
}
