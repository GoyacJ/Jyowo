import '@testing-library/jest-dom/vitest'

import { fireEvent, render, screen, waitFor } from '@testing-library/react'
import { I18nextProvider } from 'react-i18next'
import { describe, expect, it, vi } from 'vitest'

import type { ServerFrame } from '@/generated/daemon-protocol'
import type { DaemonClient } from '@/shared/daemon/client'
import { createAppI18n } from '@/shared/i18n/i18n'

import { HtmlRuntimePanel } from './HtmlRuntimePanel'

describe('HtmlRuntimePanel', () => {
  it('requires an explicit start and renders the loopback page in a restricted iframe', async () => {
    const request = vi
      .fn<DaemonClient['request']>()
      .mockResolvedValueOnce(frame('stopped'))
      .mockResolvedValueOnce(frame('ready', runtimeUrl))
      .mockResolvedValueOnce(frame('stopped'))
    renderPanel(request)

    expect(await screen.findByText('HTML source')).toBeInTheDocument()
    fireEvent.click(screen.getByRole('button', { name: 'Run HTML' }))

    const iframe = await screen.findByTitle('Preview runtime')
    expect(iframe).toHaveAttribute('src', runtimeUrl)
    expect(iframe).toHaveAttribute('sandbox', 'allow-forms allow-scripts')
    expect(iframe).toHaveAttribute('referrerpolicy', 'no-referrer')
    expect(request).toHaveBeenNthCalledWith(2, {
      command: {
        spec: { blobId, kind: 'html', title: 'Preview' },
        type: 'open',
      },
      taskId,
      type: 'runtime',
    })

    fireEvent.click(screen.getByRole('button', { name: 'Stop' }))
    await waitFor(() => expect(screen.getByRole('button', { name: 'Run HTML' })).toBeEnabled())
  })

  it('rejects runtime URLs outside the exact loopback preview route', async () => {
    const request = vi
      .fn<DaemonClient['request']>()
      .mockResolvedValue(frame('ready', 'https://example.com/preview/html-x'))
    renderPanel(request)

    expect(await screen.findByRole('button', { name: 'Run HTML' })).toBeInTheDocument()
    expect(screen.queryByTitle('Preview runtime')).not.toBeInTheDocument()
  })
})

function renderPanel(request: DaemonClient['request']) {
  return render(
    <I18nextProvider i18n={createAppI18n('en-US')}>
      <HtmlRuntimePanel
        blobId={blobId}
        client={{ request }}
        source={<p>HTML source</p>}
        taskId={taskId}
        title="Preview"
      />
    </I18nextProvider>,
  )
}

function frame(status: 'ready' | 'stopped', url?: string): ServerFrame {
  return {
    message: {
      currentUrl: null,
      error: null,
      kind: 'html',
      sessionId: `html-${blobId}`,
      status,
      taskId,
      title: 'Preview',
      type: 'runtime_session',
      view: url ? { type: 'url', url } : null,
    },
    protocolVersion: 7,
    requestId: 'runtime-request',
  }
}

const taskId = '01J00000000000000000000001'
const blobId = '01J00000000000000000000002'
const runtimeUrl = `http://127.0.0.1:43123/preview/html-${blobId}/token`
