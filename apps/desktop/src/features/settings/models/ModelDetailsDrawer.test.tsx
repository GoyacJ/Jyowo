import '@testing-library/jest-dom/vitest'

import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { act, fireEvent, render, screen, waitFor, within } from '@testing-library/react'
import type { ReactNode } from 'react'
import { describe, expect, it, vi } from 'vitest'

import { AppI18nProvider } from '@/shared/i18n/i18n'
import { uiStore } from '@/shared/state/ui-store'
import type { CommandClient } from '@/shared/tauri/commands'
import { CommandClientProvider } from '@/shared/tauri/react'
import { createTestCommandClient } from '@/testing/command-client'

import { ModelDetailsDrawer } from './ModelDetailsDrawer'
import type { ModelAssetRow } from './model-settings-view-model'

function renderDrawer(row: ModelAssetRow, client: CommandClient = createTestCommandClient()) {
  uiStore.getState().setLocale('en-US')
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false }, mutations: { retry: false } },
  })

  function Wrapper({ children }: { children: ReactNode }) {
    return (
      <CommandClientProvider client={client}>
        <QueryClientProvider client={queryClient}>
          <AppI18nProvider>{children}</AppI18nProvider>
        </QueryClientProvider>
      </CommandClientProvider>
    )
  }

  return render(<ModelDetailsDrawer onOpenChange={vi.fn()} open row={row} />, { wrapper: Wrapper })
}

describe('ModelDetailsDrawer', () => {
  it('renders required tabs and connectivity details with safe error text', () => {
    renderDrawer(failingRow)

    const dialog = screen.getByRole('dialog', { name: 'Research Claude' })
    expect(within(dialog).getByRole('tab', { name: 'Overview' })).toBeInTheDocument()
    expect(within(dialog).getByRole('tab', { name: 'Connectivity' })).toBeInTheDocument()
    expect(within(dialog).getByRole('tab', { name: 'Usage' })).toBeInTheDocument()
    expect(within(dialog).getByRole('tab', { name: 'Official quota' })).toBeInTheDocument()
    expect(within(dialog).getByRole('tab', { name: 'Configuration' })).toBeInTheDocument()
    expect(within(dialog).getByRole('tab', { name: 'Capabilities' })).toBeInTheDocument()

    fireEvent.click(within(dialog).getByRole('tab', { name: 'Connectivity' }))
    expect(dialog).toHaveTextContent('Failed')
    expect(dialog).toHaveTextContent('10,000 ms')
    expect(dialog).toHaveTextContent('Unavailable')
    expect(dialog).toHaveTextContent('2026-06-30T10:05:00Z')
    expect(dialog).toHaveTextContent('Provider returned a safe failure summary.')
  })

  it('labels usage as model-level and shows shared usage when profiles share provider/model', () => {
    renderDrawer(sharedUsageRow)

    const dialog = screen.getByRole('dialog', { name: 'Primary OpenAI' })
    fireEvent.click(within(dialog).getByRole('tab', { name: 'Usage' }))

    expect(dialog).toHaveTextContent('Model-level usage')
    expect(dialog).toHaveTextContent('Shared model usage')
    expect(dialog).toHaveTextContent('100 tokens')
    expect(dialog).toHaveTextContent('310 tokens')
    expect(dialog).toHaveTextContent('1,400 tokens')
  })

  it('renders official quota states with scope labels', () => {
    for (const [status, scope, label] of [
      ['supported', 'account', 'Account quota'],
      ['unsupported', 'provider', 'Provider quota'],
      ['failed', 'project', 'Project quota'],
      ['authRequired', 'model', 'Model quota'],
      ['notConfigured', 'account', 'Account quota'],
    ] as const) {
      const { unmount } = renderDrawer({
        ...sharedUsageRow,
        quota: {
          status,
          scope,
          scopeLabel: scope,
          sourceUrl: 'https://provider.example/docs/usage',
          fetchedAt: '2026-06-30T11:00:00Z',
          expiresAt: '2026-06-30T12:00:00Z',
          isStale: false,
          safeMessage:
            status === 'supported' || status === 'notConfigured' ? undefined : 'Safe state.',
        },
      })
      const dialog = screen.getByRole('dialog', { name: 'Primary OpenAI' })
      fireEvent.click(within(dialog).getByRole('tab', { name: 'Official quota' }))
      expect(dialog).toHaveTextContent(label)
      expect(dialog).toHaveTextContent(quotaStatusLabel[status])
      unmount()
    }
  })

  it('shows API key presence and reveals raw key only through the explicit reveal flow', async () => {
    const rawKey = 'sk-test-revealed-key'
    const requestProviderConfigApiKeyReveal = vi.fn().mockResolvedValue({
      configId: 'cfg-openai',
      expiresInSeconds: 60,
      revealToken: 'reveal-token',
      status: 'ready',
    })
    const getProviderConfigApiKey = vi.fn().mockResolvedValue({
      apiKey: rawKey,
      configId: 'cfg-openai',
    })
    renderDrawer(
      {
        ...sharedUsageRow,
        hasApiKey: true,
      },
      {
        ...createTestCommandClient(),
        requestProviderConfigApiKeyReveal,
        getProviderConfigApiKey,
      },
    )

    const dialog = screen.getByRole('dialog', { name: 'Primary OpenAI' })
    fireEvent.click(within(dialog).getByRole('tab', { name: 'Configuration' }))
    expect(dialog).toHaveTextContent('API key saved')
    expect(dialog).not.toHaveTextContent(rawKey)

    fireEvent.click(within(dialog).getByRole('button', { name: 'View key' }))
    expect(await within(dialog).findByText(rawKey)).toBeInTheDocument()
    expect(requestProviderConfigApiKeyReveal).toHaveBeenCalledWith('cfg-openai')
    expect(getProviderConfigApiKey).toHaveBeenCalledWith('cfg-openai', 'reveal-token')
  })

  it('does not apply a late reveal response after switching rows', async () => {
    const rawKey = 'sk-late-revealed-key'
    const reveal = deferred<{
      configId: string
      expiresInSeconds: number
      revealToken: string
      status: 'ready'
    }>()
    const key = deferred<{ apiKey: string; configId: string }>()
    const requestProviderConfigApiKeyReveal = vi.fn().mockReturnValue(reveal.promise)
    const getProviderConfigApiKey = vi.fn().mockReturnValue(key.promise)
    const { rerender } = renderDrawer(
      {
        ...sharedUsageRow,
        hasApiKey: true,
      },
      {
        ...createTestCommandClient(),
        requestProviderConfigApiKeyReveal,
        getProviderConfigApiKey,
      },
    )

    const dialog = screen.getByRole('dialog', { name: 'Primary OpenAI' })
    fireEvent.click(within(dialog).getByRole('tab', { name: 'Configuration' }))
    fireEvent.click(within(dialog).getByRole('button', { name: 'View key' }))

    await act(async () => {
      reveal.resolve({
        configId: 'cfg-openai',
        expiresInSeconds: 60,
        revealToken: 'reveal-token',
        status: 'ready',
      })
    })
    await waitFor(() =>
      expect(getProviderConfigApiKey).toHaveBeenCalledWith('cfg-openai', 'reveal-token'),
    )

    rerender(
      <ModelDetailsDrawer onOpenChange={vi.fn()} open row={{ ...failingRow, hasApiKey: true }} />,
    )

    await act(async () => {
      key.resolve({ apiKey: rawKey, configId: 'cfg-openai' })
    })

    const nextDialog = screen.getByRole('dialog', { name: 'Research Claude' })
    fireEvent.click(within(nextDialog).getByRole('tab', { name: 'Configuration' }))
    expect(nextDialog).not.toHaveTextContent(rawKey)
  })

  it('does not fetch the raw key when the row changes before reveal token returns', async () => {
    const reveal = deferred<{
      configId: string
      expiresInSeconds: number
      revealToken: string
      status: 'ready'
    }>()
    const requestProviderConfigApiKeyReveal = vi.fn().mockReturnValue(reveal.promise)
    const getProviderConfigApiKey = vi.fn()
    const { rerender } = renderDrawer(
      {
        ...sharedUsageRow,
        hasApiKey: true,
      },
      {
        ...createTestCommandClient(),
        requestProviderConfigApiKeyReveal,
        getProviderConfigApiKey,
      },
    )

    const dialog = screen.getByRole('dialog', { name: 'Primary OpenAI' })
    fireEvent.click(within(dialog).getByRole('tab', { name: 'Configuration' }))
    fireEvent.click(within(dialog).getByRole('button', { name: 'View key' }))

    rerender(
      <ModelDetailsDrawer onOpenChange={vi.fn()} open row={{ ...failingRow, hasApiKey: true }} />,
    )

    await act(async () => {
      reveal.resolve({
        configId: 'cfg-openai',
        expiresInSeconds: 60,
        revealToken: 'reveal-token',
        status: 'ready',
      })
    })

    expect(getProviderConfigApiKey).not.toHaveBeenCalled()
  })

  it('does not render an already revealed key after switching rows', async () => {
    const rawKey = 'sk-already-visible-key'
    const requestProviderConfigApiKeyReveal = vi.fn().mockResolvedValue({
      configId: 'cfg-openai',
      expiresInSeconds: 60,
      revealToken: 'reveal-token',
      status: 'ready',
    })
    const getProviderConfigApiKey = vi.fn().mockResolvedValue({
      apiKey: rawKey,
      configId: 'cfg-openai',
    })
    const { rerender } = renderDrawer(
      {
        ...sharedUsageRow,
        hasApiKey: true,
      },
      {
        ...createTestCommandClient(),
        requestProviderConfigApiKeyReveal,
        getProviderConfigApiKey,
      },
    )

    const dialog = screen.getByRole('dialog', { name: 'Primary OpenAI' })
    fireEvent.click(within(dialog).getByRole('tab', { name: 'Configuration' }))
    fireEvent.click(within(dialog).getByRole('button', { name: 'View key' }))
    expect(await within(dialog).findByText(rawKey)).toBeInTheDocument()

    rerender(
      <ModelDetailsDrawer onOpenChange={vi.fn()} open row={{ ...failingRow, hasApiKey: true }} />,
    )

    expect(screen.getByRole('dialog', { name: 'Research Claude' })).not.toHaveTextContent(rawKey)
  })

  it('does not apply a late reveal response after closing and reopening the same row', async () => {
    const rawKey = 'sk-late-same-config-key'
    const reveal = deferred<{
      configId: string
      expiresInSeconds: number
      revealToken: string
      status: 'ready'
    }>()
    const key = deferred<{ apiKey: string; configId: string }>()
    const requestProviderConfigApiKeyReveal = vi.fn().mockReturnValue(reveal.promise)
    const getProviderConfigApiKey = vi.fn().mockReturnValue(key.promise)
    const { rerender } = renderDrawer(
      {
        ...sharedUsageRow,
        hasApiKey: true,
      },
      {
        ...createTestCommandClient(),
        requestProviderConfigApiKeyReveal,
        getProviderConfigApiKey,
      },
    )

    const dialog = screen.getByRole('dialog', { name: 'Primary OpenAI' })
    fireEvent.click(within(dialog).getByRole('tab', { name: 'Configuration' }))
    fireEvent.click(within(dialog).getByRole('button', { name: 'View key' }))

    await act(async () => {
      reveal.resolve({
        configId: 'cfg-openai',
        expiresInSeconds: 60,
        revealToken: 'reveal-token',
        status: 'ready',
      })
    })
    await waitFor(() =>
      expect(getProviderConfigApiKey).toHaveBeenCalledWith('cfg-openai', 'reveal-token'),
    )

    rerender(<ModelDetailsDrawer onOpenChange={vi.fn()} open={false} row={sharedUsageRow} />)
    rerender(<ModelDetailsDrawer onOpenChange={vi.fn()} open row={sharedUsageRow} />)

    await act(async () => {
      key.resolve({ apiKey: rawKey, configId: 'cfg-openai' })
    })

    const reopenedDialog = screen.getByRole('dialog', { name: 'Primary OpenAI' })
    fireEvent.click(within(reopenedDialog).getByRole('tab', { name: 'Configuration' }))
    expect(reopenedDialog).not.toHaveTextContent(rawKey)
  })

  it('keeps a reopened same-config reveal session isolated from the previous request', async () => {
    const firstReveal = deferred<{
      configId: string
      expiresInSeconds: number
      revealToken: string
      status: 'ready'
    }>()
    const secondReveal = deferred<{
      configId: string
      expiresInSeconds: number
      revealToken: string
      status: 'ready'
    }>()
    const firstKey = deferred<{ apiKey: string; configId: string }>()
    const secondKey = deferred<{ apiKey: string; configId: string }>()
    const requestProviderConfigApiKeyReveal = vi
      .fn()
      .mockReturnValueOnce(firstReveal.promise)
      .mockReturnValueOnce(secondReveal.promise)
    const getProviderConfigApiKey = vi
      .fn()
      .mockReturnValueOnce(firstKey.promise)
      .mockReturnValueOnce(secondKey.promise)
    const { rerender } = renderDrawer(
      {
        ...sharedUsageRow,
        hasApiKey: true,
      },
      {
        ...createTestCommandClient(),
        requestProviderConfigApiKeyReveal,
        getProviderConfigApiKey,
      },
    )

    const dialog = screen.getByRole('dialog', { name: 'Primary OpenAI' })
    fireEvent.click(within(dialog).getByRole('tab', { name: 'Configuration' }))
    fireEvent.click(within(dialog).getByRole('button', { name: 'View key' }))
    await act(async () => {
      firstReveal.resolve({
        configId: 'cfg-openai',
        expiresInSeconds: 60,
        revealToken: 'first-token',
        status: 'ready',
      })
    })
    await waitFor(() =>
      expect(getProviderConfigApiKey).toHaveBeenCalledWith('cfg-openai', 'first-token'),
    )

    rerender(<ModelDetailsDrawer onOpenChange={vi.fn()} open={false} row={sharedUsageRow} />)
    rerender(<ModelDetailsDrawer onOpenChange={vi.fn()} open row={sharedUsageRow} />)
    const reopenedDialog = screen.getByRole('dialog', { name: 'Primary OpenAI' })
    fireEvent.click(within(reopenedDialog).getByRole('tab', { name: 'Configuration' }))
    fireEvent.click(within(reopenedDialog).getByRole('button', { name: 'View key' }))
    await act(async () => {
      secondReveal.resolve({
        configId: 'cfg-openai',
        expiresInSeconds: 60,
        revealToken: 'second-token',
        status: 'ready',
      })
      secondKey.resolve({ apiKey: 'sk-second-session', configId: 'cfg-openai' })
    })

    expect(await within(reopenedDialog).findByText('sk-second-session')).toBeInTheDocument()

    await act(async () => {
      firstKey.resolve({ apiKey: 'sk-first-session', configId: 'cfg-openai' })
    })

    expect(reopenedDialog).toHaveTextContent('sk-second-session')
    expect(reopenedDialog).not.toHaveTextContent('sk-first-session')
  })

  it('renders backend catalog model capabilities when available', () => {
    renderDrawer(sharedUsageRow)

    const dialog = screen.getByRole('dialog', { name: 'Primary OpenAI' })
    fireEvent.click(within(dialog).getByRole('tab', { name: 'Capabilities' }))

    expect(dialog).toHaveTextContent('Streaming')
    expect(dialog).toHaveTextContent('Tools')
    expect(dialog).toHaveTextContent('Structured output')
    expect(dialog).toHaveTextContent('128,000')
    expect(dialog).toHaveTextContent('8,192')
  })
})

const sharedUsageRow: ModelAssetRow = {
  configId: 'cfg-openai',
  providerId: 'openai',
  modelId: 'gpt-4.1',
  modelDescriptor: {
    protocol: 'responses',
    conversationCapability: {
      inputModalities: ['text'],
      outputModalities: ['text'],
      contextWindow: 128000,
      maxOutputTokens: 8192,
      streaming: true,
      toolCalling: true,
      reasoning: false,
      promptCache: false,
      structuredOutput: true,
    },
    contextWindow: 128000,
    displayName: 'GPT-4.1',
    lifecycle: { kind: 'stable' },
    maxOutputTokens: 8192,
    modelId: 'gpt-4.1',
    runtimeStatus: { kind: 'runnable' },
  },
  displayName: 'Primary OpenAI',
  providerDisplayName: 'OpenAI',
  isDefault: true,
  hasApiKey: true,
  connectivity: {
    status: 'online',
    checkedAt: '2026-06-30T10:00:00Z',
    latencyMs: 118,
    timeoutMs: 10_000,
  },
  usage: {
    status: 'ready',
    sharedModelUsage: true,
    today: usage(70, 30),
    monthToDate: usage(220, 90),
    allTime: usage(900, 500),
  },
  quota: {
    status: 'supported',
    scope: 'account',
    scopeLabel: 'account',
    sourceUrl: 'https://platform.openai.com/docs/api-reference/usage',
    fetchedAt: '2026-06-30T11:00:00Z',
    expiresAt: '2026-06-30T12:00:00Z',
    isStale: false,
    quotaUsed: 10,
    quotaTotal: 100,
    quotaRemaining: 90,
    unit: 'usd',
  },
}

const failingRow: ModelAssetRow = {
  ...sharedUsageRow,
  configId: 'cfg-anthropic',
  providerId: 'anthropic',
  modelId: 'claude-sonnet',
  displayName: 'Research Claude',
  providerDisplayName: 'Anthropic',
  isDefault: false,
  connectivity: {
    status: 'failed',
    checkedAt: '2026-06-30T10:05:00Z',
    timeoutMs: 10_000,
    errorKind: 'provider',
    safeMessage: 'Provider returned a safe failure summary.',
  },
}

const quotaStatusLabel = {
  supported: 'Supported',
  unsupported: 'Unsupported',
  failed: 'Failed',
  authRequired: 'Auth required',
  notConfigured: 'Not configured',
}

function usage(inputTokens: number, outputTokens: number) {
  return {
    cacheReadTokens: 0,
    cacheWriteTokens: 0,
    costMicros: 0,
    inputTokens,
    outputTokens,
    toolCalls: 0,
  }
}

function deferred<T>() {
  let resolve: (value: T) => void = () => undefined
  const promise = new Promise<T>((resolvePromise) => {
    resolve = resolvePromise
  })
  return { promise, resolve }
}
