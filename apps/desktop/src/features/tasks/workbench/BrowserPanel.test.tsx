import '@testing-library/jest-dom/vitest'

import { fireEvent, render, screen } from '@testing-library/react'
import { I18nextProvider } from 'react-i18next'
import { describe, expect, it, vi } from 'vitest'

import type { ServerFrame, ServerMessage } from '@/generated/daemon-protocol'
import type { DaemonClient } from '@/shared/daemon/client'
import { createAppI18n } from '@/shared/i18n/i18n'

import { BrowserPanel } from './BrowserPanel'

describe('BrowserPanel', () => {
  it('opens a stopped task browser and embeds only its local dashboard', async () => {
    const request = vi
      .fn()
      .mockResolvedValueOnce(frame(session('stopped')))
      .mockResolvedValueOnce(
        frame(
          session('ready', {
            currentUrl: 'https://example.com/',
            dashboardUrl: 'http://127.0.0.1:43121/',
            title: 'Example Domain',
          }),
        ),
      )

    renderPanel(request)

    const browser = await screen.findByTitle('Task browser')
    expect(browser).toHaveAttribute('src', 'http://127.0.0.1:43121/')
    expect(browser).toHaveAttribute(
      'sandbox',
      'allow-downloads allow-forms allow-pointer-lock allow-same-origin allow-scripts',
    )
    expect(request).toHaveBeenNthCalledWith(1, {
      command: { type: 'status' },
      taskId,
      type: 'browser',
    })
    expect(request).toHaveBeenNthCalledWith(2, {
      command: { type: 'open' },
      taskId,
      type: 'browser',
    })
    expect(screen.getByText('Example Domain')).toBeInTheDocument()
    expect(screen.getByText('https://example.com/')).toBeInTheDocument()
  })

  it('stops the browser without closing the workbench', async () => {
    const request = vi
      .fn()
      .mockResolvedValueOnce(frame(session('ready', { dashboardUrl: 'http://127.0.0.1:43121' })))
      .mockResolvedValueOnce(frame(session('stopped')))

    renderPanel(request)

    fireEvent.click(await screen.findByRole('button', { name: 'Stop browser' }))

    expect(await screen.findByText('Browser stopped')).toBeInTheDocument()
    expect(request).toHaveBeenLastCalledWith({
      command: { type: 'close' },
      taskId,
      type: 'browser',
    })
  })

  it('reports an unavailable bundled runtime without creating an iframe', async () => {
    const request = vi.fn().mockResolvedValue(
      frame(
        session('unavailable', {
          unavailableReason: 'bundled Node.js executable was not found',
        }),
      ),
    )

    renderPanel(request)

    expect(await screen.findByText('Browser unavailable')).toBeInTheDocument()
    expect(screen.getByText('bundled Node.js executable was not found')).toBeInTheDocument()
    expect(screen.queryByTitle('Task browser')).not.toBeInTheDocument()
  })
})

function renderPanel(request: Pick<DaemonClient, 'request'>['request']) {
  return render(
    <I18nextProvider i18n={createAppI18n('en-US')}>
      <BrowserPanel client={{ request }} taskId={taskId} />
    </I18nextProvider>,
  )
}

function frame(message: ServerMessage): ServerFrame {
  return { message, protocolVersion: 4, requestId: 'browser-request' }
}

function session(
  status: Extract<ServerMessage, { type: 'browser_session' }>['status'],
  overrides: Partial<Extract<ServerMessage, { type: 'browser_session' }>> = {},
): Extract<ServerMessage, { type: 'browser_session' }> {
  return {
    status,
    taskId,
    type: 'browser_session',
    ...overrides,
  }
}

const taskId = '01J00000000000000000000001'
