import { defineConfig, devices } from '@playwright/test'

const webPort = Number(process.env.PLAYWRIGHT_WEB_PORT ?? 5174)

if (!Number.isInteger(webPort) || webPort < 1 || webPort > 65535) {
  throw new Error('PLAYWRIGHT_WEB_PORT must be an integer port between 1 and 65535.')
}

const webUrl = `http://127.0.0.1:${webPort}`

export default defineConfig({
  testDir: './e2e',
  testIgnore: /conversation-evidence-storybook\.spec\.ts/,
  fullyParallel: true,
  reporter: [['list']],
  use: {
    baseURL: webUrl,
    trace: 'retain-on-failure',
  },
  webServer: {
    command: `VITE_JYOWO_E2E_COMMAND_CLIENT=fixture pnpm exec vite --host 127.0.0.1 --port ${webPort} --strictPort`,
    url: webUrl,
    reuseExistingServer: false,
  },
  projects: [
    {
      name: 'chromium',
      use: { ...devices['Desktop Chrome'] },
    },
  ],
})
