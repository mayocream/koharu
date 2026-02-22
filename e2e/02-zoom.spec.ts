import { expect, test, type Locator, type Page } from '@playwright/test'
import {
  enableFileInputFallback,
  FIXTURE_IMAGE_PATHS,
  getWorkspaceViewport,
  importImages,
  openFirstDocument,
  readZoomPercent,
} from './helpers'

const MIN_ZOOM = 10
const MAX_ZOOM = 100

function clampZoom(value: number) {
  return Math.max(MIN_ZOOM, Math.min(MAX_ZOOM, value))
}

async function dispatchCtrlWheelStep(
  page: Page,
  viewport: Locator,
  deltaY: number,
) {
  const box = await viewport.boundingBox()
  if (!box) {
    throw new Error('workspace viewport is not visible')
  }
  await page.mouse.move(box.x + box.width / 2, box.y + box.height / 2)
  await page.keyboard.down('Control')
  await page.mouse.wheel(0, deltaY)
  await page.keyboard.up('Control')
  await page.waitForTimeout(220)
}

test.beforeEach(async ({ page }) => {
  await enableFileInputFallback(page)
})

test('ctrl + wheel updates zoom by expected step each event', async ({
  page,
}) => {
  await page.goto('/')
  await importImages(page, [FIXTURE_IMAGE_PATHS[0]])
  await openFirstDocument(page)

  const viewport = await getWorkspaceViewport(page)
  const zoomOutDelta = 20
  const zoomInDelta = -20

  let currentZoom = await readZoomPercent(page)
  for (let i = 0; i < 4; i += 1) {
    const expected = clampZoom(currentZoom - 1)
    await dispatchCtrlWheelStep(page, viewport, zoomOutDelta)
    await expect.poll(async () => readZoomPercent(page)).toBe(expected)
    currentZoom = expected
  }

  for (let i = 0; i < 4; i += 1) {
    const expected = clampZoom(currentZoom + 1)
    await dispatchCtrlWheelStep(page, viewport, zoomInDelta)
    await expect.poll(async () => readZoomPercent(page)).toBe(expected)
    currentZoom = expected
  }
})
