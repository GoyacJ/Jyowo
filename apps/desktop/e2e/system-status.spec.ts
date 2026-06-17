import { expect, test } from '@playwright/test'

test('renders system status in the web mock runtime', async ({ page }) => {
  await page.goto('/')

  const statusPanel = page.locator('section[aria-labelledby="app-title"]')

  await expect(statusPanel.getByRole('heading', { name: 'Jyowo' })).toBeVisible()
  await expect(statusPanel.getByText('0.1.0')).toBeVisible()
  await expect(statusPanel.getByText('tauri2-react')).toBeVisible()
  await expect(statusPanel.getByText('jyowo_harness_sdk').first()).toBeVisible()
  await expect(statusPanel.getByText('available')).toBeVisible()
})
