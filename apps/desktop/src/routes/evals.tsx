import { createFileRoute } from '@tanstack/react-router'

import { EvalLabPage } from '@/features/evals/EvalLabPage'

export const Route = createFileRoute('/evals')({
  component: EvalLabPage,
})
