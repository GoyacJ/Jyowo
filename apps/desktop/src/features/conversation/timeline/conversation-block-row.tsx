import { memo } from 'react'

import { ConversationBlockRenderer } from './conversation-block-renderer'
import type { ConversationBlock } from './conversation-blocks'

export const ConversationBlockRow = memo(function ConversationBlockRow({
  block,
  onPermissionResolve,
  onReviewContinue,
}: {
  block: ConversationBlock
  onPermissionResolve?: (request: {
    conversationId: string
    requestId: string
    decision: 'approve' | 'deny'
  }) => void
  onReviewContinue?: (prompt: string) => void
}) {
  return (
    <div data-conversation-block-id={block.id} id={`conversation-block-${block.id}`}>
      <ConversationBlockRenderer
        block={block}
        onPermissionResolve={onPermissionResolve}
        onReviewContinue={onReviewContinue}
      />
    </div>
  )
})
