import AxeBuilder from '@axe-core/playwright'
import { expect, type Page, test } from '@playwright/test'

test('queue, composer, artifacts, and statuses are keyboard reachable', async ({ page }) => {
  await openStory(page, 'active-streaming')

  const liveStatus = page.getByRole('status').filter({ hasText: /Task update:|任务更新：/ })
  await expect(liveStatus).toContainText('2 files changed, 18 insertions')
  await expect(page.getByText(/2 queued|2 条排队消息/).first()).toBeVisible()

  const queueAction = page.getByRole('button', {
    name: /^(?:Edit queued message 1|编辑第 1 条排队消息)$/,
  })
  await focusByTab(page, queueAction)
  await expect(queueAction).toBeFocused()
  await expect(queueAction).toHaveCSS('outline-style', 'solid')

  const composer = page.getByRole('textbox').last()
  await focusByTab(page, composer)
  await expect(composer).toBeFocused()

  const details = page.locator('summary').first()
  await focusByTab(page, details)
  await page.keyboard.press('Enter')
  await expect(details.locator('..')).toHaveAttribute('open', '')
})

test('permission decision is keyboard reachable and announced in text', async ({ page }) => {
  await openStory(page, 'permission-waiting')

  await expect(page.getByText(/^(?:Waiting permission|等待权限)$/).first()).toBeVisible()
  await expect(
    page.getByRole('status', { name: /Pending permission request|待处理的权限请求/ }),
  ).toContainText(
    /Permission request: cargo test -p jyowo-harness-daemon|权限请求：cargo test -p jyowo-harness-daemon/,
  )
  const permission = page.getByRole('button', { name: 'Allow once' })
  await focusByTab(page, permission)
  await expect(permission).toBeFocused()
  await page.keyboard.press('Enter')
  await expect(
    page.getByRole('status').filter({ hasText: /Submitting Allow once|正在提交 Allow once/ }),
  ).toBeVisible()
})

test('workbench tabs implement arrow-key navigation', async ({ page }) => {
  await openStory(page, 'open-workbench')

  const report = page.getByRole('tab', { name: 'recovery-report.md' })
  await focusByTab(page, report)
  await page.keyboard.press('ArrowLeft')
  await expect(page.getByRole('tab', { name: '2 files changed, 18 insertions' })).toBeFocused()
  await expect(page.getByRole('tab', { name: '2 files changed, 18 insertions' })).toHaveAttribute(
    'aria-selected',
    'true',
  )
})

test('narrow workbench replaces the readable timeline', async ({ page }) => {
  await openStory(page, 'open-workbench', { height: 760, width: 600 })

  await expect(page.getByTestId('task-reading-column')).toHaveAttribute('aria-hidden', 'true')
  await expect(page.getByTestId('task-workbench')).toHaveAttribute('data-layout', 'fullscreen')
})

test('workspace container width controls workbench overlay mode', async ({ page }) => {
  await openStory(page, 'open-workbench', { height: 900, width: 1280 })
  await page.locator('main').evaluate((element) => {
    element.style.width = '900px'
  })

  await expect(page.getByTestId('task-workbench')).toHaveAttribute('data-layout', 'overlay')
})

test('wide workbench stays inside its workspace container', async ({ page }) => {
  await openStory(page, 'open-workbench', { height: 900, width: 1280 })
  const main = page.locator('main')
  await main.evaluate((element) => {
    element.style.width = '1100px'
  })
  await expect(page.getByTestId('task-workbench')).toHaveAttribute('data-layout', 'docked')

  const mainBox = await main.boundingBox()
  const workbench = await page.getByTestId('task-workbench').boundingBox()

  expect(mainBox).not.toBeNull()
  expect(workbench).not.toBeNull()
  expect((workbench?.x ?? 0) + (workbench?.width ?? 0)).toBeLessThanOrEqual(
    (mainBox?.x ?? 0) + (mainBox?.width ?? 0),
  )
})

for (const story of [
  'idle-task',
  'active-streaming',
  'permission-waiting',
  'failed-command-large-diff',
  'interrupted-recovery',
  'open-workbench',
] as const) {
  test(`${story} has no automated accessibility violations`, async ({ page }) => {
    await openStory(page, story)

    const results = await new AxeBuilder({ page }).include('main').analyze()
    expect(results.violations, JSON.stringify(results.violations, null, 2)).toEqual([])
  })
}

async function openStory(page: Page, story: string, viewport = { height: 760, width: 900 }) {
  await page.setViewportSize(viewport)
  await page.emulateMedia({ colorScheme: 'light', reducedMotion: 'reduce' })
  await page.goto(`/iframe.html?id=tasks-task-workspace--${story}&viewMode=story`)
  await page.waitForLoadState('networkidle')
  await expect(page.locator('main')).toBeVisible()
}

async function focusByTab(page: Page, target: ReturnType<Page['getByRole']>) {
  for (let index = 0; index < 80; index += 1) {
    await page.keyboard.press('Tab')
    if (await target.evaluate((element) => element === document.activeElement)) return
  }
  throw new Error('Target was not reachable within 80 Tab presses')
}
