import { expect, test } from './helpers/test'
import { PIPELINE_SINGLE, bootstrapApp, importAndOpenPage } from './helpers/app'
import {
  drawStrokeOnCanvas,
  readCanvasNonBlackCoverage,
  readImageSrc,
  waitForImageSrcChange,
} from './helpers/canvas'
import { prepareDetectAndOcr, runInpaint } from './helpers/pipeline'
import { selectors } from './helpers/selectors'

test.beforeEach(async ({ page }) => {
  await bootstrapApp(page)
})

test('editing mask with repair brush updates inpainted result', async ({
  page,
}) => {
  await importAndOpenPage(page, PIPELINE_SINGLE)
  await prepareDetectAndOcr(page)
  await runInpaint(page)

  await page.getByTestId(selectors.panels.textBlockCard(0)).click()

  const annotation = page
    .getByTestId(selectors.workspace.annotations)
    .locator('.border-primary')
    .first()

  let centerRatio = { x: 0.35, y: 0.25 }
  if ((await annotation.count()) > 0) {
    const annotationBox = await annotation.boundingBox()
    const canvasBox = await page
      .getByTestId(selectors.workspace.canvas)
      .boundingBox()
    if (annotationBox && canvasBox) {
      centerRatio = {
        x: (annotationBox.x + annotationBox.width / 2 - canvasBox.x) / canvasBox.width,
        y: (annotationBox.y + annotationBox.height / 2 - canvasBox.y) / canvasBox.height,
      }
    }
  }

  const inpaintedImage = page.getByTestId(selectors.workspace.inpaintedImage)
  await expect(inpaintedImage).toBeVisible()
  const beforeSrc = await readImageSrc(inpaintedImage)

  await page.getByTestId(selectors.tools.repairBrush).click()
  const maskCanvas = page.getByTestId(selectors.workspace.maskCanvas)
  await expect(maskCanvas).toBeVisible()
  const coverageBefore = await readCanvasNonBlackCoverage(maskCanvas)

  await drawStrokeOnCanvas(
    page,
    maskCanvas,
    { x: Math.max(0.05, centerRatio.x - 0.03), y: Math.max(0.05, centerRatio.y - 0.03) },
    { x: Math.min(0.95, centerRatio.x + 0.05), y: Math.min(0.95, centerRatio.y + 0.03) },
  )

  await expect
    .poll(async () => readCanvasNonBlackCoverage(maskCanvas), { timeout: 60_000 })
    .not.toBe(coverageBefore)

  await waitForImageSrcChange(inpaintedImage, beforeSrc, 180_000)
})
