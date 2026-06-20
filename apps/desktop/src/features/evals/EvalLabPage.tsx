import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { useTranslation } from 'react-i18next'

import { useCommandClient } from '@/shared/tauri/react'
import { type EvalCase, EvalLab } from './EvalLab'

const evalCasesQueryKey = ['eval-cases'] as const

export function EvalLabPage() {
  const { t } = useTranslation('evals')
  const commandClient = useCommandClient()
  const queryClient = useQueryClient()
  const evalCasesQuery = useQuery({
    queryFn: () => commandClient.listEvalCases(),
    queryKey: evalCasesQueryKey,
  })
  const runEvalCase = useMutation({
    mutationFn: (caseId: string) => commandClient.runEvalCase(caseId),
    onSuccess: (response) => {
      queryClient.setQueryData<{ cases: EvalCase[] }>(evalCasesQueryKey, (current) => ({
        cases: (current?.cases ?? []).map((evalCase) =>
          evalCase.id === response.case.id ? response.case : evalCase,
        ),
      }))
    },
  })
  const cases = evalCasesQuery.data?.cases ?? []

  return (
    <div className="mx-auto flex w-full max-w-5xl flex-col gap-5">
      <header>
        <h1 className="font-semibold text-2xl">{t('pageTitle')}</h1>
        <p className="mt-1 text-muted-foreground text-sm">{t('pageDescription')}</p>
      </header>

      {evalCasesQuery.isLoading ? (
        <p className="text-muted-foreground text-sm">{t('loading')}</p>
      ) : null}
      <EvalLab
        cases={cases}
        errorMessage={evalCasesQuery.error || runEvalCase.error ? 'unavailable' : undefined}
        onRunCase={(caseId) => {
          runEvalCase.mutate(caseId)
        }}
      />
    </div>
  )
}
