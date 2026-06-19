import { createFileRoute } from '@tanstack/react-router'

import { ArtifactsPage } from '@/features/artifacts/ArtifactsPage'

export const Route = createFileRoute('/artifacts')({
  component: ArtifactsPage,
})
