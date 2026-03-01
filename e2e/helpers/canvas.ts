import { expect, type Locator, type Page } from '@playwright/test'
import { selectors } from './selectors'

const MIN_ZOOM = 10
const MAX_ZOOM = 100

export function clampZoom(value: number) {
  return Math.max(MIN_ZOOM, Math.min(MAX_ZOOM, value))
}

export async function readZoomPercent(page: Page) {
  const value = await page
    .getByTestId(selectors.zoom.slider)
    .locator('[role="slider"]')
    .first()
    .getAttribute('aria-valuenow')
  const parsed = Number(value)
  if (!Number.isFinite(parsed)) {
    throw new Error(`Unable to parse zoom slider value: ${String(value)}`)
  }
  return parsed
}

export async function ctrlWheelZoomStep(
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
}

export async function dragZoomSliderTo(page: Page, targetPercent: number) {
  const slider = page.getByTestId(selectors.zoom.slider)
  const track = slider.locator('[data-slot="slider-track"]').first()
  const thumb = slider.locator('[data-slot="slider-thumb"]').first()
  const box = (await track.boundingBox()) ?? (await slider.boundingBox())
  if (!box) {
    throw new Error('zoom slider is not visible')
  }

  const clamped = clampZoom(targetPercent)
  const ratio = (clamped - MIN_ZOOM) / (MAX_ZOOM - MIN_ZOOM)
  const x = box.x + Math.round(box.width * ratio)
  const y = box.y + box.height / 2

  const thumbBox = await thumb.boundingBox()
  if (!thumbBox) {
    await page.mouse.click(x, y)
    return
  }

  await page.mouse.move(
    thumbBox.x + thumbBox.width / 2,
    thumbBox.y + thumbBox.height / 2,
  )
  await page.mouse.down()
  await page.mouse.move(x, y, { steps: 8 })
  await page.mouse.up()
}

type PointRatio = { x: number; y: number }

export async function drawStrokeOnCanvas(
  page: Page,
  canvas: Locator,
  start: PointRatio = { x: 0.3, y: 0.3 },
  end: PointRatio = { x: 0.65, y: 0.55 },
) {
  const box = await canvas.boundingBox()
  if (!box) {
    throw new Error('canvas is not visible')
  }

  const startX = box.x + box.width * start.x
  const startY = box.y + box.height * start.y
  const endX = box.x + box.width * end.x
  const endY = box.y + box.height * end.y

  await page.mouse.move(startX, startY)
  await page.mouse.down()
  await page.mouse.move(endX, endY, { steps: 10 })
  await page.mouse.up()
}

export async function readCanvasInkCoverage(canvas: Locator) {
  return canvas.evaluate((node) => {
    if (!(node instanceof HTMLCanvasElement)) return -1
    if (node.width === 0 || node.height === 0) return 0
    const ctx = node.getContext('2d')
    if (!ctx) return -1
    const { data } = ctx.getImageData(0, 0, node.width, node.height)
    let count = 0
    for (let i = 3; i < data.length; i += 4) {
      if (data[i] > 0) count += 1
    }
    return count
  })
}

export async function readCanvasNonBlackCoverage(canvas: Locator) {
  return canvas.evaluate((node) => {
    if (!(node instanceof HTMLCanvasElement)) return -1
    if (node.width === 0 || node.height === 0) return 0
    const ctx = node.getContext('2d')
    if (!ctx) return -1
    const { data } = ctx.getImageData(0, 0, node.width, node.height)
    let count = 0
    for (let i = 0; i < data.length; i += 4) {
      const r = data[i]
      const g = data[i + 1]
      const b = data[i + 2]
      if (r + g + b > 0) count += 1
    }
    return count
  })
}

export async function readImageSrc(locator: Locator) {
  return locator.evaluateAll((nodes) => {
    const src = nodes
      .map((node) => node.getAttribute('src'))
      .find((value): value is string => Boolean(value))
    return src ?? null
  })
}

export async function waitForImageSrcChange(
  locator: Locator,
  previousSrc: string | null,
  timeout = 120_000,
) {
  await expect
    .poll(async () => {
      const srcs = await locator.evaluateAll((nodes) =>
        nodes
          .map((node) => node.getAttribute('src'))
          .filter((value): value is string => Boolean(value)),
      )
      return srcs.some((src) => src !== previousSrc)
    }, { timeout })
    .toBe(true)
}
