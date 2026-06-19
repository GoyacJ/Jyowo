import { expect, test } from '@playwright/test'

test('command palette is keyboard usable and restores focus', async ({ page }) => {
  await page.goto('/')

  const sidebarSearch = page.getByRole('searchbox', { name: 'Search' })
  await sidebarSearch.focus()

  await page.keyboard.press('ControlOrMeta+K')

  const dialog = page.getByRole('dialog', { name: 'Command palette' })
  const commandSearch = page.getByRole('combobox', { name: 'Search commands' })

  await expect(dialog).toBeVisible()
  await expect(commandSearch).toBeFocused()
  await expect(page.getByRole('option', { name: 'New conversation' })).toBeVisible()

  await page.keyboard.press('Escape')

  await expect(dialog).toBeHidden()
  await expect(sidebarSearch).toBeFocused()

  await page.keyboard.press('ControlOrMeta+K')
  await commandSearch.fill('activity')
  await page.keyboard.press('Enter')

  await expect(dialog).toBeHidden()
  await expect(page.getByRole('region', { name: 'Activity' })).toHaveAttribute(
    'data-expanded',
    'true',
  )
})

test('command palette opens eval support route', async ({ page }) => {
  await page.goto('/')

  await page.getByRole('searchbox', { name: 'Search' }).focus()
  await page.keyboard.press('ControlOrMeta+K')
  await page.getByRole('combobox', { name: 'Search commands' }).fill('evals')
  await page.keyboard.press('Enter')

  await expect(page).toHaveURL(/\/evals$/)
  await expect(page.getByRole('heading', { name: 'Evals' })).toBeVisible()
  await expect(page.getByRole('heading', { name: 'Regression smoke' })).toBeVisible()
})
