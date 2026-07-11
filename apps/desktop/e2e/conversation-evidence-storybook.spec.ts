import { expect, type Page, test } from '@playwright/test'

type ThemeTarget = 'dark' | 'light' | 'system'

const storyTargets: Array<{
  expectedText: string | RegExp
  id: string
  name: string
  requiresDiff?: boolean
  requiresShell?: boolean
  theme: ThemeTarget
}> = [
  {
    expectedText: '红测和失败证据已经保留，下一步修复实现。',
    id: 'conversation-timeline--codex-evidence-flow',
    name: 'CodexEvidenceFlow system',
    requiresDiff: true,
    requiresShell: true,
    theme: 'system',
  },
  {
    expectedText: '红测和失败证据已经保留，下一步修复实现。',
    id: 'conversation-timeline--codex-evidence-flow',
    name: 'CodexEvidenceFlow light',
    requiresDiff: true,
    requiresShell: true,
    theme: 'light',
  },
  {
    expectedText: '红测和失败证据已经保留，下一步修复实现。',
    id: 'conversation-timeline--codex-evidence-flow',
    name: 'CodexEvidenceFlow dark',
    requiresDiff: true,
    requiresShell: true,
    theme: 'dark',
  },
  {
    expectedText: '退出码 1',
    id: 'conversation-timeline--codex-evidence-failed-command',
    name: 'CodexEvidenceFailedCommand dark',
    requiresShell: true,
    theme: 'dark',
  },
  {
    expectedText: 'ConversationTimeline.test.tsx',
    id: 'conversation-timeline--codex-evidence-large-diff',
    name: 'CodexEvidenceLargeDiff light',
    requiresDiff: true,
    theme: 'light',
  },
  {
    expectedText: /Permission|权限|Install dependencies/,
    id: 'conversation-timeline--codex-evidence-permission-pending',
    name: 'CodexEvidencePermissionPending dark',
    theme: 'dark',
  },
]

for (const target of storyTargets) {
  test(`${target.name} renders evidence without visual regressions`, async ({ page }) => {
    await openStory(page, target.id, target.theme)

    await expect(page.getByText(target.expectedText).first()).toBeVisible()
    await expectNoFrameworkOverlay(page)
    await expectMeaningfulBody(page)
    await expectTimelineStaysInReadingColumn(page)

    if (target.requiresDiff) {
      const diffDisclosure = page
        .locator('[data-evidence-disclosure-id]')
        .filter({ has: page.getByRole('button', { name: /diff/iu }) })
        .getByRole('button', { expanded: false })
        .first()
      if ((await diffDisclosure.count()) > 0) {
        await diffDisclosure.click()
      }
      await expectEvidenceScrollRegion(page, 'diff-scroll-region')
    }
    if (target.requiresShell) {
      await expectEvidenceScrollRegion(page, 'command-output-scroll-region')
      await expect(page.getByText(/(?:exit|退出码) 1/).first()).toBeVisible()
    }

    const copyButton = page.getByRole('button', { name: /Copy|复制/ }).first()
    if ((await copyButton.count()) > 0) {
      await copyButton.focus()
      await expect(copyButton).toBeFocused()
    }
  })
}

async function openStory(page: Page, storyId: string, theme: ThemeTarget) {
  await page.setViewportSize({ width: 1280, height: 900 })
  await page.emulateMedia({ colorScheme: theme === 'light' ? 'light' : 'dark' })
  await page.goto(`/iframe.html?id=${storyId}&viewMode=story`)
  await page.waitForLoadState('networkidle')
  await applyTheme(page, theme)
  await expect(page.locator('main')).toBeVisible()
}

async function applyTheme(page: Page, theme: ThemeTarget) {
  await page.evaluate((nextTheme) => {
    const useDark =
      nextTheme === 'dark' ||
      (nextTheme === 'system' && window.matchMedia('(prefers-color-scheme: dark)').matches)
    document.documentElement.classList.toggle('dark', useDark)
    document.documentElement.dataset.theme = nextTheme
    for (const main of document.querySelectorAll('main')) {
      main.classList.toggle('dark', useDark)
    }
  }, theme)
}

async function expectNoFrameworkOverlay(page: Page) {
  await expect(
    page.getByText(
      /Pre-transform error|Internal server error|Failed to fetch dynamically imported module/,
    ),
  ).toHaveCount(0)
}

async function expectMeaningfulBody(page: Page) {
  const bodyText = await page.locator('body').innerText()
  expect(bodyText.length).toBeGreaterThan(200)
}

async function expectTimelineStaysInReadingColumn(page: Page) {
  const timeline = page.getByTestId('conversation-timeline-scroll-content').first()
  await expect(timeline).toBeVisible()
  const box = await timeline.boundingBox()
  expect(box).not.toBeNull()
  expect(box?.width).toBeLessThanOrEqual(985)
}

async function expectEvidenceScrollRegion(page: Page, testId: string) {
  const region = page.getByTestId(testId).first()
  await expect(region).toBeVisible()
  const regionBox = await region.boundingBox()
  const timelineBox = await page
    .getByTestId('conversation-timeline-scroll-content')
    .first()
    .boundingBox()

  expect(regionBox).not.toBeNull()
  expect(timelineBox).not.toBeNull()
  expect(regionBox?.width).toBeLessThanOrEqual((timelineBox?.width ?? 0) + 1)
  expect(regionBox?.height).toBeGreaterThan(10)
}
