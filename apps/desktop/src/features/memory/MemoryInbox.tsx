import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { Check, GitMerge, X } from 'lucide-react'
import { useState } from 'react'
import { useTranslation } from 'react-i18next'

import {
  approveMemoryCandidate,
  DEFAULT_MEMORY_TENANT_ID,
  listMemoryCandidates,
  mergeMemoryCandidate,
  rejectMemoryCandidate,
} from '@/shared/tauri/commands'
import { useCommandClient } from '@/shared/tauri/react'
import { Button } from '@/shared/ui/button'
import { Card, CardContent, CardHeader, CardTitle } from '@/shared/ui/card'

const inboxQueryKeys = {
  all: ['memory-inbox'] as const,
}

export function MemoryInbox() {
  const { t } = useTranslation('memory')
  const commandClient = useCommandClient()
  const queryClient = useQueryClient()
  const [message, setMessage] = useState<string | null>(null)
  const [selectedCandidateIds, setSelectedCandidateIds] = useState<string[]>([])

  const inboxQuery = useQuery({
    queryKey: inboxQueryKeys.all,
    queryFn: () =>
      listMemoryCandidates({ limit: 50, tenantId: DEFAULT_MEMORY_TENANT_ID }, commandClient),
  })

  const approveMutation = useMutation({
    mutationFn: (candidateId: string) =>
      approveMemoryCandidate({ candidateId, tenantId: DEFAULT_MEMORY_TENANT_ID }, commandClient),
    onSuccess: () => {
      setMessage(t('candidateApproved'))
      queryClient.invalidateQueries({ queryKey: inboxQueryKeys.all })
    },
  })

  const rejectMutation = useMutation({
    mutationFn: (candidateId: string) =>
      rejectMemoryCandidate(
        {
          candidateId,
          reason: 'rejected by user',
          tenantId: DEFAULT_MEMORY_TENANT_ID,
        },
        commandClient,
      ),
    onSuccess: () => {
      setMessage(t('candidateRejected'))
      queryClient.invalidateQueries({ queryKey: inboxQueryKeys.all })
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
      return mergeMemoryCandidate(
        {
          candidateIds: selectedCandidates.map((candidate) => candidate.id),
          evidence: firstCandidate.evidence,
          mergedRecord: {
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
                    (candidate) => candidate.proposed_record.metadata.tags,
                  ),
                ),
              ),
              ttl: firstCandidate.proposed_record.metadata.ttl ?? null,
            },
            visibility: firstCandidate.proposed_record.visibility,
          },
          tenantId: DEFAULT_MEMORY_TENANT_ID,
        },
        commandClient,
      )
    },
    onSuccess: () => {
      setMessage(t('candidateMerged'))
      setSelectedCandidateIds([])
      queryClient.invalidateQueries({ queryKey: inboxQueryKeys.all })
    },
  })

  if (inboxQuery.isLoading) {
    return <div className="p-4 text-muted-foreground">{t('loading')}</div>
  }
  if (inboxQuery.isError) {
    return <div className="p-4 text-destructive">{t('errorLoading')}</div>
  }

  const candidates = inboxQuery.data?.candidates ?? []
  const selectedCount = selectedCandidateIds.length

  if (candidates.length === 0) {
    return <div className="p-4 text-muted-foreground">{t('inboxEmpty')}</div>
  }

  return (
    <div className="space-y-4 p-4">
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
              <input
                aria-label={t('selectCandidate')}
                checked={selectedCandidateIds.includes(candidate.id)}
                disabled={candidate.state !== 'proposed'}
                type="checkbox"
                onChange={(event) => {
                  setSelectedCandidateIds((currentIds) =>
                    event.target.checked
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
    </div>
  )
}

function formatMemoryKind(kind: string | { custom: string }) {
  return typeof kind === 'string' ? kind : kind.custom
}
