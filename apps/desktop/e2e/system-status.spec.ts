import { expect, test } from '@playwright/test'

test('runs the browser E2E task flow through the daemon fixture client', async ({ page }) => {
  await page.goto('/')

  const taskNavigation = page.getByRole('navigation', { name: '会话' })
  await expect(taskNavigation).toBeVisible()

  const recoveredTask = taskNavigation.getByRole('button', {
    exact: true,
    name: 'Daemon recovery before restart',
  })
  await expect(recoveredTask).toBeVisible()
  await recoveredTask.click()

  await expect(page.getByRole('heading', { name: 'Daemon recovery before restart' })).toBeVisible()
  await expect(page.getByText('已连接', { exact: true })).toBeVisible()
  await expect(
    page.getByTestId('task-timeline-scroll-content').getByText('Renderer bridge recovery evidence'),
  ).toBeVisible()
})
