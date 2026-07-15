import { expect, type Page, test } from '@playwright/test'

type Theme = 'dark' | 'light'

const states = [
  'idle-task',
  'active-streaming',
  'permission-waiting',
  'failed-command-large-diff',
  'interrupted-recovery',
  'open-workbench',
] as const

for (const viewport of [
  { height: 900, label: 'wide', width: 1280 },
  { height: 760, label: 'narrow', width: 900 },
] as const) {
  for (const state of states) {
    for (const theme of ['light', 'dark'] as const) {
      test(`${state} matches the ${viewport.label} ${theme} baseline`, async ({ page }) => {
        await openWorkspaceStory(page, state, theme, viewport)

        await expect(page.getByRole('heading', { name: 'Verify daemon recovery' })).toBeVisible()
        await expect(page.locator('main')).toHaveScreenshot(
          `${state}-${viewport.label}-${theme}.png`,
        )
      })
    }
  }
}

async function openWorkspaceStory(
  page: Page,
  story: string,
  theme: Theme,
  viewport: { height: number; width: number },
) {
  await page.setViewportSize(viewport)
  await page.emulateMedia({ colorScheme: theme, reducedMotion: 'reduce' })
  await page.addInitScript(() => {
    Date.now = () => Date.parse('2026-07-11T06:01:30Z')
  })
  await page.addInitScript((preference) => {
    localStorage.setItem('jyowo-ui-theme', preference)
  }, theme)
  await page.goto(`/iframe.html?id=tasks-task-workspace--${story}&viewMode=story`)
  await page.waitForLoadState('networkidle')
  await page.evaluate((preference) => {
    document.documentElement.classList.toggle('dark', preference === 'dark')
    document.documentElement.dataset.theme = preference
  }, theme)
  await expect(page.locator('main')).toBeVisible()
  if (story === 'open-workbench' && viewport.width < 1024) {
    await page
      .getByRole('complementary', { name: /Task workbench|任务工作台/ })
      .scrollIntoViewIfNeeded()
  }
}
