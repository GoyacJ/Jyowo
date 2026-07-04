import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { Check, Trash2, X } from 'lucide-react'
import { useState } from 'react'
import { useTranslation } from 'react-i18next'

import {
  listMemoryCandidates,
  approveMemoryCandidate,
  rejectMemoryCandidate,
  type ListMemoryCandidatesRequest,
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

  const inboxQuery = useQuery({
    queryKey: inboxQueryKeys.all,
    queryFn: () => listMemoryCandidates({ limit: 50 }, commandClient),
  })

  const approveMutation = useMutation({
    mutationFn: (candidateId: string) =>
      approveMemoryCandidate({ candidateId } as any, commandClient),
    onSuccess: () => {
      setMessage(t('candidateApproved'))
      queryClient.invalidateQueries({ queryKey: inboxQueryKeys.all })
    },
  })

  const rejectMutation = useMutation({
    mutationFn: (candidateId: string) =>
      rejectMemoryCandidate(
        { candidateId, reason: 'rejected by user' } as any,
        commandClient,
      ),
    onSuccess: () => {
      setMessage(t('candidateRejected'))
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

  if (candidates.length === 0) {
    return <div className="p-4 text-muted-foreground">{t('inboxEmpty')}</div>
  }

  return (
    <div className="space-y-4 p-4">
      {message && (
        <div className="rounded bg-green-50 p-2 text-sm text-green-700">
          {message}
        </div>
      )}
      {candidates.map((candidate) => (
        <Card key={candidate.id}>
          <CardHeader>
            <CardTitle className="text-sm">
              {candidate.proposed_record.kind} · {candidate.state}
            </CardTitle>
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
