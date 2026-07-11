export function UserMessage({ content }: { content: string }) {
  return (
    <div
      className="ml-auto max-w-[min(78%,38rem)] break-words rounded-2xl rounded-br-md bg-user-bubble px-4 py-2.5 text-sm leading-6 text-foreground shadow-sm"
      data-testid="user-message"
    >
      {content}
    </div>
  )
}
