import { expect, test } from '@playwright/test'

test('renders the conversation workspace in the web mock runtime', async ({ page }) => {
  await page.goto('/')

  const workspaceNav = page.getByRole('navigation', { name: 'Workspace' })

  await expect(workspaceNav).toBeVisible()
  await expect(workspaceNav.getByRole('searchbox', { name: 'Search' })).toBeVisible()
  await expect(page.getByRole('heading', { name: 'Build the desktop foundation' })).toBeVisible()
  await expect(page.getByPlaceholder('Ask Jyowo anything about this project...')).toBeVisible()
})

test('supports the main conversation work path in the web mock runtime', async ({ page }) => {
  await page.goto('/')

  await page.getByPlaceholder('Ask Jyowo anything about this project...').fill('Continue the setup')
  await page.getByRole('button', { name: 'Send message' }).click()

  await expect(page.getByText('Continue the setup')).toBeVisible()
  await expect(page.getByText('Plan')).toBeVisible()
  await expect(page.getByText('Working: run')).toBeVisible()
  await expect(page.getByText('Review generated foundation')).toBeVisible()
  await expect(page.getByRole('button', { name: 'Continue' })).toBeVisible()

  await page.getByRole('button', { name: 'View all activity' }).click()

  await expect(page.getByRole('region', { name: 'Activity' })).toHaveAttribute(
    'data-expanded',
    'true',
  )
  await expect(page.getByRole('region', { name: 'Usage summary' })).toBeVisible()
  await expect(page.getByRole('region', { name: 'Replay timeline' })).toBeVisible()
})
