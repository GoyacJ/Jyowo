import { useUiStore } from '@/shared/state/ui-store'
import type { TimelineRenderBlock } from './timeline-render-blocks'

export function useTimelineBlockDisclosure({
  block,
  conversationId,
  runId,
}: {
  block: Extract<TimelineRenderBlock, { defaultOpen: boolean; forcedOpen: boolean }>
  conversationId: string
  runId: string
}) {
  const disclosureId = `conversation:${conversationId}:run:${runId}:block:${block.kind}:${block.id}`
  const storedOpen = useUiStore((state) => state.evidenceDisclosureOpen[disclosureId])
  const setDisclosureOpen = useUiStore((state) => state.setEvidenceDisclosureOpen)
  const open = block.forcedOpen || (storedOpen ?? block.defaultOpen)

  return {
    open,
    setOpen: (nextOpen: boolean) => setDisclosureOpen(disclosureId, nextOpen),
  }
}
