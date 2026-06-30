import { expect, type Page, test } from '@playwright/test'

const storyTargets = [
  {
    id: 'settings-modelsettingspage--ready',
    name: 'ready',
    viewport: { width: 1440, height: 900 },
  },
  {
    id: 'settings-modelsettingspage--partial-data',
    name: 'partial data',
    viewport: { width: 1024, height: 768 },
  },
  {
    id: 'settings-modelsettingspage--empty',
    name: 'empty',
    viewport: { width: 1440, height: 900 },
  },
  {
    id: 'settings-modelsettingspage--narrow-layout',
    name: 'narrow',
    viewport: { width: 390, height: 844 },
  },
]

for (const target of storyTargets) {
  test(`ModelSettingsPage ${target.name} story renders as a stable control surface`, async ({
    page,
  }) => {
    await openStory(page, target.id, target.viewport)

    await expect(page.getByRole('heading', { exact: true, name: 'Models' })).toBeVisible()
    await expect(page.getByLabel('Model settings summary')).toBeVisible()
    await expect(page.getByLabel('Model filters')).toBeVisible()
    await expect(page.getByLabel('Model matrix')).toBeVisible()
    await expect(page.getByText('Model configuration details')).toHaveCount(0)
    await expectNoFrameworkOverlay(page)
    await expectNoHorizontalPageOverflow(page)
    await expectNoVisibleTextOverlap(page)
    await expectFirstViewportContainsControlSurface(page)

    const screenshot = await page.screenshot({ fullPage: false })
    expect(screenshot.length).toBeGreaterThan(10_000)
  })
}

async function openStory(page: Page, storyId: string, viewport: { width: number; height: number }) {
  await page.setViewportSize(viewport)
  await page.goto(`/iframe.html?id=${storyId}&viewMode=story`)
  await page.waitForLoadState('networkidle')
  await expect(page.locator('main')).toBeVisible()
}

async function expectNoFrameworkOverlay(page: Page) {
  await expect(
    page.getByText(
      /Pre-transform error|Internal server error|Failed to fetch dynamically imported module/,
    ),
  ).toHaveCount(0)
}

async function expectNoHorizontalPageOverflow(page: Page) {
  const overflow = await page.evaluate(
    () => document.documentElement.scrollWidth > window.innerWidth,
  )
  expect(overflow).toBe(false)
}

async function expectNoVisibleTextOverlap(page: Page) {
  const overlaps = await page.evaluate(() => {
    const elements = Array.from(document.body.querySelectorAll<HTMLElement>('*'))
    const textBoxes = elements
      .filter((element) => {
        const style = window.getComputedStyle(element)
        const text = element.innerText?.trim()
        const hasTextChild = Array.from(element.children).some(
          (child) =>
            child instanceof HTMLElement &&
            window.getComputedStyle(child).display !== 'none' &&
            child.innerText.trim().length > 0,
        )
        return (
          text &&
          !hasTextChild &&
          style.visibility !== 'hidden' &&
          style.display !== 'none' &&
          Number(style.opacity) !== 0
        )
      })
      .map((element) => {
        const rect = element.getBoundingClientRect()
        return {
          text: element.innerText.trim(),
          rect: {
            bottom: rect.bottom,
            height: rect.height,
            left: rect.left,
            right: rect.right,
            top: rect.top,
            width: rect.width,
          },
        }
      })
      .filter(({ rect }) => rect.width > 1 && rect.height > 1)

    const failures: string[] = []

    for (let index = 0; index < textBoxes.length; index += 1) {
      for (let nextIndex = index + 1; nextIndex < textBoxes.length; nextIndex += 1) {
        const first = textBoxes[index]
        const second = textBoxes[nextIndex]
        const overlapX =
          Math.min(first.rect.right, second.rect.right) -
          Math.max(first.rect.left, second.rect.left)
        const overlapY =
          Math.min(first.rect.bottom, second.rect.bottom) -
          Math.max(first.rect.top, second.rect.top)

        if (overlapX > 2 && overlapY > 2) {
          failures.push(`${first.text} overlaps ${second.text}`)
        }
      }
    }

    return failures
  })

  expect(overlaps).toEqual([])
}

async function expectFirstViewportContainsControlSurface(page: Page) {
  const summary = await page.getByLabel('Model settings summary').boundingBox()
  const filters = await page.getByLabel('Model filters').boundingBox()
  const matrix = await page.getByLabel('Model matrix').boundingBox()

  expect(summary).not.toBeNull()
  expect(filters).not.toBeNull()
  expect(matrix).not.toBeNull()
  expect((summary?.y ?? Number.POSITIVE_INFINITY) + (summary?.height ?? 0)).toBeLessThan(844)
  expect((filters?.y ?? Number.POSITIVE_INFINITY) + (filters?.height ?? 0)).toBeLessThan(844)
  expect(matrix?.y ?? Number.POSITIVE_INFINITY).toBeLessThan(844)
}
