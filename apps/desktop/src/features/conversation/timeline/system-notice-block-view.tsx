import type { SystemNoticeBlock } from './conversation-blocks'

export function SystemNoticeBlockView({ block }: { block: SystemNoticeBlock }) {
  return (
    <section
      className="ml-12 border-border border-l pl-4 text-sm data-[tone=warning]:text-warning"
      data-tone={block.tone}
    >
      {block.message}
    </section>
  )
}
