import { expect, test } from '@playwright/test'

const taskId = '01J00000000000000000000051'
const assistantReply = 'Protocol reply is visible once.'

test('projects a submitted message from raw daemon engine events', async ({ page }) => {
  const browserErrors: string[] = []
  page.on('console', (message) => {
    if (message.type() === 'error') browserErrors.push(message.text())
  })
  page.on('pageerror', (error) => browserErrors.push(error.message))

  await page.goto(`/?taskId=${taskId}`)

  await page.getByPlaceholder('向 Jyowo 询问这个项目…').fill('Return the protocol fixture reply')
  await page.getByRole('button', { name: '发送消息' }).click()

  const reply = page.locator('[data-narrative="true"]').filter({ hasText: assistantReply })
  await expect(reply).toHaveCount(1)
  await expect(reply).toHaveText(assistantReply)
  await expect(
    page.getByTestId('task-reading-column').locator('header').getByText('已完成', { exact: true }),
  ).toBeVisible()
  await expect(page.getByText(/assistant delta produced/i)).toHaveCount(0)
  await expect(page.getByText(/engine\.run_(started|ended)/i)).toHaveCount(0)

  expect(browserErrors).toEqual([])
})
