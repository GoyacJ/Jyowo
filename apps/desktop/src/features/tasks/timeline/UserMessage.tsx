export function UserMessage({ content }: { content: string }) {
  return (
    <div
      className="ml-auto max-w-[min(78%,38rem)] rounded-2xl rounded-br-md bg-muted px-4 py-2.5 text-sm leading-6 text-foreground shadow-sm"
      data-testid="user-message"
    >
      {content}
    </div>
  )
}
