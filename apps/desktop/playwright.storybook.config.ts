import { defineConfig, devices } from '@playwright/test'

const storybookPort = Number(process.env.PLAYWRIGHT_STORYBOOK_PORT ?? 6007)

if (!Number.isInteger(storybookPort) || storybookPort < 1 || storybookPort > 65535) {
  throw new Error('PLAYWRIGHT_STORYBOOK_PORT must be an integer port between 1 and 65535.')
}

const storybookUrl = `http://127.0.0.1:${storybookPort}`

export default defineConfig({
  testDir: './e2e',
  testMatch: /(conversation-evidence-storybook|model-settings-storybook)\.spec\.ts/,
  fullyParallel: false,
  reporter: [['list']],
  use: {
    baseURL: storybookUrl,
    trace: 'retain-on-failure',
  },
  webServer: {
    command: `pnpm exec storybook dev --host 127.0.0.1 --port ${storybookPort} --ci --no-open`,
    url: storybookUrl,
    reuseExistingServer: false,
  },
  projects: [
    {
      name: 'chromium',
      use: { ...devices['Desktop Chrome'] },
    },
  ],
})
