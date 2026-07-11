import '@fontsource-variable/inter/index.css'

import React from 'react'
import ReactDOM from 'react-dom/client'

import App from '@/app/App'
import type { DaemonClient } from '@/shared/daemon/client'
import '@/shared/styles/global.css'
import type { CommandClient } from '@/shared/tauri/commands'

async function createE2eClients(): Promise<{
  commandClient?: CommandClient
  daemonClient?: DaemonClient
}> {
  if (import.meta.env.VITE_JYOWO_E2E_COMMAND_CLIENT !== 'fixture') {
    return {}
  }

  const [{ createTestCommandClient }, { createE2eDaemonClient }] = await Promise.all([
    import('@/testing/command-client'),
    import('@/testing/daemon-client'),
  ])
  return {
    commandClient: createTestCommandClient(),
    daemonClient: createE2eDaemonClient(),
  }
}

void createE2eClients().then(({ commandClient, daemonClient }) => {
  ReactDOM.createRoot(document.getElementById('root') as HTMLElement).render(
    <React.StrictMode>
      <App commandClient={commandClient} daemonClient={daemonClient} />
    </React.StrictMode>,
  )
})
