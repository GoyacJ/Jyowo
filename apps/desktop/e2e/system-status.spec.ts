import { expect, type Page, test } from '@playwright/test'

test('runs the browser E2E conversation flow through the fixture client', async ({ page }) => {
  await page.goto('/')

  const workspaceNav = page.getByRole('navigation', { name: '工作区' })
  await expect(workspaceNav).toBeVisible()

  const existingConversation = workspaceNav.getByRole('button', {
    name: 'Build the desktop foundation Restore the product shell',
  })
  await expect(existingConversation).toBeVisible()
  await existingConversation.click()
  await expect(page.getByRole('heading', { name: 'Build the desktop foundation' })).toBeVisible()

  const permissionPanel = page.locator('[data-permission-request-id]').first()
  await expect(permissionPanel).toBeVisible()
  await approveFirstPermission(page)
  await expect(page.getByText(/权限：提交中|Awaiting approval/).first()).toBeVisible()
  await expect(page.getByRole('button', { name: /Approve|批准/ }).first()).toBeDisabled()

  await workspaceNav.getByRole('button', { name: '新建对话' }).click()
  await expect(page.getByRole('heading', { name: '新建对话' })).toBeVisible()

  await page.getByRole('textbox').fill('Verify the conversation runtime path')
  await page.getByRole('button', { name: /Send message|发送消息/ }).click()

  await expect(page.getByText('Drafting the implementation plan.')).toBeVisible()
  await expect(page.getByRole('button', { name: /工具 已运行 2 条工具|Ran 2 tools/ })).toBeVisible()
  await expect(page.getByText('Desktop foundation created')).toBeVisible()
  await expect(page.getByText('The setup is ready for review.')).toBeVisible()
})

async function approveFirstPermission(page: Page) {
  const approveButton = page.getByRole('button', { name: /Approve|批准/ }).first()
  await expect(approveButton).toBeVisible()
  await approveButton.click()
}
