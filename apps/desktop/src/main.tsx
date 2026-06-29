import '@fontsource-variable/inter/index.css'

import React from 'react'
import ReactDOM from 'react-dom/client'

import App from '@/app/App'
import '@/shared/styles/global.css'
import type { CommandClient } from '@/shared/tauri/commands'

async function createE2eCommandClient(): Promise<CommandClient | undefined> {
  if (import.meta.env.VITE_JYOWO_E2E_COMMAND_CLIENT !== 'fixture') {
    return undefined
  }

  const { createTestCommandClient } = await import('@/testing/command-client')
  return createTestCommandClient()
}

void createE2eCommandClient().then((commandClient) => {
  ReactDOM.createRoot(document.getElementById('root') as HTMLElement).render(
    <React.StrictMode>
      <App commandClient={commandClient} />
    </React.StrictMode>,
  )
})
