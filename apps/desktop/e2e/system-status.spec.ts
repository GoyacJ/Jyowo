import { expect, test } from '@playwright/test'

test('runs the browser E2E task flow through the daemon fixture client', async ({ page }) => {
  await page.goto('/')

  const taskNavigation = page.getByRole('navigation', { name: 'Tasks' })
  await expect(taskNavigation).toBeVisible()

  const recoveredTask = taskNavigation.getByRole('button', {
    name: 'Daemon recovery before restart Completed',
  })
  await expect(recoveredTask).toBeVisible()
  await recoveredTask.click()

  await expect(page.getByRole('heading', { name: 'Daemon recovery before restart' })).toBeVisible()
  await expect(page.getByText('Connected', { exact: true })).toBeVisible()
  await expect(page.getByText('Renderer bridge recovery evidence')).toBeVisible()
})
