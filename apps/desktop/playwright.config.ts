import { defineConfig, devices } from '@playwright/test'

const webMockPort = Number(process.env.PLAYWRIGHT_WEB_MOCK_PORT ?? 5174)

if (!Number.isInteger(webMockPort) || webMockPort < 1 || webMockPort > 65535) {
  throw new Error('PLAYWRIGHT_WEB_MOCK_PORT must be an integer port between 1 and 65535.')
}

const webMockUrl = `http://127.0.0.1:${webMockPort}`

export default defineConfig({
  testDir: './e2e',
  fullyParallel: true,
  reporter: [['list']],
  use: {
    baseURL: webMockUrl,
    trace: 'retain-on-failure',
  },
  webServer: {
    command: `pnpm exec vite --host 127.0.0.1 --port ${webMockPort} --strictPort`,
    url: webMockUrl,
    reuseExistingServer: false,
    env: {
      VITE_JYOWO_COMMAND_CLIENT: 'mock',
    },
  },
  projects: [
    {
      name: 'chromium',
      use: { ...devices['Desktop Chrome'] },
    },
  ],
})
