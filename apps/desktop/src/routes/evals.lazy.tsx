import { createLazyFileRoute } from '@tanstack/react-router'

import { EvalLabPage } from '@/features/evals/EvalLabPage'

export const Route = createLazyFileRoute('/evals')({
  component: EvalLabPage,
})
