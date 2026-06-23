import { expect, test } from '@playwright/test'

test('command palette is keyboard usable and restores focus', async ({ page }) => {
  await page.goto('/')

  const commandButton = page.getByRole('button', { name: '打开命令面板' })
  await commandButton.focus()

  await page.keyboard.press('ControlOrMeta+K')

  const dialog = page.getByRole('dialog', { name: '命令面板' })
  const commandSearch = page.getByRole('combobox', { name: '搜索命令' })

  await expect(dialog).toBeVisible()
  await expect(commandSearch).toBeFocused()
  await expect(page.getByRole('option', { name: '新建对话' })).toBeVisible()

  await page.keyboard.press('Escape')

  await expect(dialog).toBeHidden()
  await expect(commandButton).toBeFocused()

  await page.keyboard.press('ControlOrMeta+K')
  await commandSearch.fill('设置')
  await page.keyboard.press('Enter')

  await expect(dialog).toBeHidden()
  await expect(page).toHaveURL(/\/settings$/)
  await expect(page.getByRole('region', { name: '设置' })).toBeVisible()
})

test('command palette opens eval support route', async ({ page }) => {
  await page.goto('/')

  await page.getByRole('button', { name: '打开命令面板' }).focus()
  await page.keyboard.press('ControlOrMeta+K')
  await page.getByRole('combobox', { name: '搜索命令' }).fill('评测')
  await page.keyboard.press('Enter')

  await expect(page).toHaveURL(/\/evals$/)
  await expect(page.getByRole('heading', { name: '评测', exact: true })).toBeVisible()
  await expect(page.getByRole('heading', { name: 'Regression smoke' })).toBeVisible()
})
