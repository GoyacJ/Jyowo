export const conversationQueryKeys = {
  all: ['conversation'] as const,
  detail: (workspacePath: string, conversationId: string) =>
    [...conversationQueryKeys.all, 'detail', workspacePath, conversationId] as const,
  list: (workspacePath: string) => [...conversationQueryKeys.all, 'list', workspacePath] as const,
}
