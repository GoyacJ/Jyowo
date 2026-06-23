import { expect, test } from '@playwright/test'

test('renders the conversation workspace in the web mock runtime', async ({ page }) => {
  await page.goto('/')

  const workspaceNav = page.getByRole('navigation', { name: '工作区' })

  await expect(workspaceNav).toBeVisible()
  await expect(page.getByRole('heading', { name: 'Build the desktop foundation' })).toBeVisible()
  await expect(page.getByPlaceholder('向 Jyowo 询问这个项目...')).toBeVisible()
})

test('supports the main conversation work path in the web mock runtime', async ({ page }) => {
  await page.goto('/')

  await page.getByPlaceholder('向 Jyowo 询问这个项目...').fill('Continue the setup')
  await page.getByRole('button', { name: '发送消息' }).click()

  await expect(page.getByText('Continue the setup')).toBeVisible()
  await expect(page.getByText('Drafting the implementation plan.')).toBeVisible()
  await expect(page.getByText('Reading files')).toBeVisible()
  await expect(page.getByText('Run local verification')).toBeVisible()
  const permissionBlock = page.locator('section').filter({ hasText: 'Run local verification' })
  await permissionBlock.getByRole('button', { name: 'Approve' }).click()
  await expect(permissionBlock.getByText('Approved')).toBeVisible()
  await expect(permissionBlock.getByRole('button', { name: 'Approve' })).toBeDisabled()
  await expect(page.getByText('Desktop foundation created')).toBeVisible()
  await expect(page.getByText('The setup is ready for review.')).toBeVisible()

  await expect(page.getByRole('region', { name: '状态' })).toBeVisible()
})
