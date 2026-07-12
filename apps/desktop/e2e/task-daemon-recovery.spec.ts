import { expect, type Page, test } from '@playwright/test'

const taskId = '01J00000000000000000000051'
const blobId = '01J00000000000000000000052'

type RecoveryTelemetry = {
  blobReads: Array<{ blobId: string; generation: number }>
  deliveries: Array<{ afterOffset: number; generation: number; offsets: number[] }>
  subscriptions: Array<{ afterOffset: number; generation: number }>
}

test('renderer bridge resumes a recovered task without replaying committed offsets', async ({
  page,
}) => {
  // This exercises the renderer/bridge recovery contract. Process-level sidecar restart and
  // Windows Named Pipe recovery remain platform CI gates because browser E2E runs against Vite.
  await page.goto(`/?taskId=${taskId}`)

  await expect(page.getByRole('heading', { name: 'Daemon recovery before restart' })).toBeVisible()
  await expect(page.getByText('已连接', { exact: true })).toBeVisible()
  await expect(committedEvent(page)).toBeVisible()

  await page.getByRole('button', { name: '打开更改' }).click()
  await expect(page.getByText('renderer bridge output loaded by blob id')).toBeVisible()

  await controlFixture(page, 'stop')
  await page.reload()
  await expect(page.getByText('任务不可用', { exact: true })).toBeVisible()

  await controlFixture(page, 'restart')
  await page.reload()

  await expect(page.getByRole('heading', { name: 'Daemon recovery after restart' })).toBeVisible()
  await expect(page.getByText('已连接', { exact: true })).toBeVisible()
  await expect(committedEvent(page)).toBeVisible()

  await page.getByRole('button', { name: '打开更改' }).click()
  await expect(page.getByText('renderer bridge output loaded by blob id')).toBeVisible()

  const telemetry = await fixtureTelemetry(page)
  const recoveredSubscriptions = telemetry.subscriptions.filter((entry) => entry.generation === 1)
  const recoveredDeliveries = telemetry.deliveries.filter((entry) => entry.generation === 1)

  expect(recoveredSubscriptions.length).toBeGreaterThan(0)
  expect(recoveredSubscriptions.every((entry) => entry.afterOffset === 3)).toBe(true)
  expect(recoveredDeliveries).toEqual([])
  expect(
    telemetry.deliveries.every((delivery) =>
      delivery.offsets.every((offset) => offset > delivery.afterOffset),
    ),
  ).toBe(true)
  expect(telemetry.blobReads.length).toBeGreaterThanOrEqual(2)
  expect(telemetry.blobReads.every((entry) => entry.blobId === blobId)).toBe(true)
  expect(
    telemetry.blobReads.every(
      (entry) => Object.keys(entry).sort().join(',') === 'blobId,generation',
    ),
  ).toBe(true)
})

async function controlFixture(page: Page, action: 'restart' | 'stop') {
  await page.evaluate((nextAction) => {
    const fixture = Reflect.get(window, '__JYOWO_E2E_DAEMON__') as
      | { restart: () => void; stop: () => void }
      | undefined
    if (!fixture) throw new Error('E2E daemon recovery fixture is unavailable')
    fixture[nextAction]()
  }, action)
}

async function fixtureTelemetry(page: Page) {
  return page.evaluate(() => {
    const fixture = Reflect.get(window, '__JYOWO_E2E_DAEMON__') as
      | { telemetry: () => RecoveryTelemetry }
      | undefined
    if (!fixture) throw new Error('E2E daemon recovery fixture is unavailable')
    return fixture.telemetry()
  })
}

function committedEvent(page: Page) {
  return page.getByTestId('timeline-item').filter({ hasText: /committed event delivered once/i })
}
