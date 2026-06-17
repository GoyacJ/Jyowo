import { createContext, type ReactNode, useContext } from 'react'

import type { CommandClient } from './commands'

const CommandClientContext = createContext<CommandClient | null>(null)

export function CommandClientProvider({
  children,
  client,
}: {
  children: ReactNode
  client: CommandClient
}) {
  return <CommandClientContext.Provider value={client}>{children}</CommandClientContext.Provider>
}

export function useCommandClient() {
  const client = useContext(CommandClientContext)

  if (!client) {
    throw new Error('CommandClientProvider is missing')
  }

  return client
}
