import { createContext, type ReactNode, useContext } from 'react'

import type { DaemonClient } from '@/shared/daemon/client'
import type { CommandClient } from './commands'

const CommandClientContext = createContext<CommandClient | null>(null)
const DaemonClientContext = createContext<DaemonClient | null>(null)

export function CommandClientProvider({
  children,
  client,
}: {
  children: ReactNode
  client: CommandClient
}) {
  return <CommandClientContext.Provider value={client}>{children}</CommandClientContext.Provider>
}

export function DaemonClientProvider({
  children,
  client,
}: {
  children: ReactNode
  client: DaemonClient
}) {
  return <DaemonClientContext.Provider value={client}>{children}</DaemonClientContext.Provider>
}

export function useCommandClient() {
  const client = useContext(CommandClientContext)

  if (!client) {
    throw new Error('CommandClientProvider is missing')
  }

  return client
}

export function useDaemonClient() {
  const client = useContext(DaemonClientContext)

  if (!client) {
    throw new Error('DaemonClientProvider is missing')
  }

  return client
}
