import { useEffect, useState } from 'react'

import {
  getAppInfo,
  getHarnessHealthcheck,
  type AppInfo,
  type HarnessHealthcheck,
} from './tauri/client'

type LoadState =
  | { status: 'loading' }
  | { status: 'ready'; appInfo: AppInfo; healthcheck: HarnessHealthcheck }
  | { status: 'error'; message: string }

function getErrorMessage(error: unknown): string {
  return error instanceof Error ? error.message : String(error)
}

export default function App() {
  const [state, setState] = useState<LoadState>({ status: 'loading' })

  useEffect(() => {
    let active = true

    Promise.all([getAppInfo(), getHarnessHealthcheck()])
      .then(([appInfo, healthcheck]) => {
        if (active) {
          setState({ status: 'ready', appInfo, healthcheck })
        }
      })
      .catch((error: unknown) => {
        if (active) {
          setState({ status: 'error', message: getErrorMessage(error) })
        }
      })

    return () => {
      active = false
    }
  }, [])

  return (
    <main className="app-shell">
      <section className="status-panel" aria-labelledby="app-title">
        {state.status === 'loading' ? (
          <p className="status-text">Loading Jyowo</p>
        ) : null}

        {state.status === 'error' ? (
          <>
            <h1 id="app-title">Jyowo</h1>
            <p className="status-text">{state.message}</p>
          </>
        ) : null}

        {state.status === 'ready' ? (
          <>
            <div className="title-row">
              <h1 id="app-title">{state.appInfo.name}</h1>
              <span>{state.healthcheck.status}</span>
            </div>
            <dl className="info-grid">
              <div>
                <dt>Version</dt>
                <dd>{state.appInfo.version}</dd>
              </div>
              <div>
                <dt>Shell</dt>
                <dd>{state.appInfo.shell}</dd>
              </div>
              <div>
                <dt>SDK crate</dt>
                <dd>{state.appInfo.harness.sdkCrate}</dd>
              </div>
              <div>
                <dt>Mode</dt>
                <dd>{state.appInfo.harness.mode}</dd>
              </div>
            </dl>
          </>
        ) : null}
      </section>
    </main>
  )
}
