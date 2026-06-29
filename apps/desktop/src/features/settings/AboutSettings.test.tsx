import '@testing-library/jest-dom/vitest'

import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { act, fireEvent, render, screen, waitFor } from '@testing-library/react'
import type { ReactNode } from 'react'
import { beforeEach, describe, expect, it, vi } from 'vitest'

import { AppI18nProvider } from '@/shared/i18n/i18n'
import { uiStore } from '@/shared/state/ui-store'
import { createMockCommandClient } from '@/shared/tauri/mock-client'
import { CommandClientProvider } from '@/shared/tauri/react'
import { checkForAppUpdate, downloadAndInstallUpdate, relaunchApp } from '@/shared/tauri/updater'

import { AboutSettings } from './AboutSettings'

vi.mock('@/shared/tauri/updater', () => ({
  checkForAppUpdate: vi.fn(),
  downloadAndInstallUpdate: vi.fn(),
  relaunchApp: vi.fn(),
}))

function deferred<T>() {
  let resolve!: (value: T) => void
  let reject!: (error: unknown) => void
  const promise = new Promise<T>((promiseResolve, promiseReject) => {
    resolve = promiseResolve
    reject = promiseReject
  })

  return { promise, reject, resolve }
}

function renderAboutSettings() {
  const queryClient = new QueryClient({
    defaultOptions: {
      queries: { retry: false },
    },
  })

  function Wrapper({ children }: { children: ReactNode }) {
    return (
      <CommandClientProvider client={createMockCommandClient()}>
        <QueryClientProvider client={queryClient}>
          <AppI18nProvider>{children}</AppI18nProvider>
        </QueryClientProvider>
      </CommandClientProvider>
    )
  }

  return render(<AboutSettings />, { wrapper: Wrapper })
}

describe('AboutSettings', () => {
  beforeEach(() => {
    uiStore.getState().setLocale('zh-CN')
    vi.mocked(checkForAppUpdate).mockReset()
    vi.mocked(downloadAndInstallUpdate).mockReset()
    vi.mocked(relaunchApp).mockReset()
  })

  it('shows current app version before update checks', async () => {
    renderAboutSettings()

    expect(await screen.findByRole('heading', { name: '关于 Jyowo' })).toBeInTheDocument()
    expect(screen.getByText('当前版本')).toBeInTheDocument()
    expect(await screen.findByText('0.1.0')).toBeInTheDocument()
    expect(screen.getByText('未检查')).toBeInTheDocument()
    expect(screen.getByRole('button', { name: '检查更新' })).toBeInTheDocument()
  })

  it('shows checking then up-to-date when no update is available', async () => {
    const check = deferred<{ kind: 'current' }>()
    vi.mocked(checkForAppUpdate).mockReturnValueOnce(check.promise)

    renderAboutSettings()
    fireEvent.click(await screen.findByRole('button', { name: '检查更新' }))

    expect(await screen.findAllByText('检查中')).toHaveLength(2)

    await act(async () => {
      check.resolve({ kind: 'current' })
    })

    expect(await screen.findByText('已是最新')).toBeInTheDocument()
  })

  it('shows available update metadata and renders release notes as text', async () => {
    vi.mocked(checkForAppUpdate).mockResolvedValueOnce({
      kind: 'available',
      update: {
        body: '<script>alert(1)</script>\n- 更新日志',
        currentVersion: '0.1.0',
        handle: {
          currentVersion: '0.1.0',
          downloadAndInstall: vi.fn(),
          version: '0.2.0',
        },
        version: '0.2.0',
      },
    })

    const { container } = renderAboutSettings()
    fireEvent.click(await screen.findByRole('button', { name: '检查更新' }))

    expect(await screen.findByText('有新版本')).toBeInTheDocument()
    expect(screen.getByText('0.2.0')).toBeInTheDocument()
    expect(screen.getByText(/<script>alert\(1\)<\/script>/)).toBeInTheDocument()
    expect(container.querySelector('script')).toBeNull()
  })

  it('shows download progress, installed state, and relaunches after install', async () => {
    const install = deferred<void>()
    vi.mocked(checkForAppUpdate).mockResolvedValueOnce({
      kind: 'available',
      update: {
        currentVersion: '0.1.0',
        handle: {
          currentVersion: '0.1.0',
          downloadAndInstall: vi.fn(),
          version: '0.2.0',
        },
        version: '0.2.0',
      },
    })
    vi.mocked(downloadAndInstallUpdate).mockImplementationOnce(async (_update, onProgress) => {
      onProgress?.({ contentLength: 100, downloadedBytes: 0, kind: 'started' })
      onProgress?.({ contentLength: 100, downloadedBytes: 50, kind: 'progress' })
      return install.promise
    })
    vi.mocked(relaunchApp).mockResolvedValueOnce()

    renderAboutSettings()
    fireEvent.click(await screen.findByRole('button', { name: '检查更新' }))
    fireEvent.click(await screen.findByRole('button', { name: '下载并安装' }))

    expect(await screen.findByText('下载中')).toBeInTheDocument()
    expect(screen.getByRole('progressbar', { name: '更新下载进度' })).toHaveAttribute(
      'aria-valuenow',
      '50',
    )

    await act(async () => {
      install.resolve()
    })

    expect(await screen.findByText('已安装待重启')).toBeInTheDocument()
    await waitFor(() => expect(relaunchApp).toHaveBeenCalledOnce())
  })

  it('shows check failure details', async () => {
    vi.mocked(checkForAppUpdate).mockRejectedValueOnce(new Error('offline'))

    renderAboutSettings()
    fireEvent.click(await screen.findByRole('button', { name: '检查更新' }))

    expect(await screen.findByText('检查失败')).toBeInTheDocument()
    expect(screen.getByText('offline')).toBeInTheDocument()
  })
})
