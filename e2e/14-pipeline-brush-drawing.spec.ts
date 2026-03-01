import { expect, test } from './helpers/test'
import {
  PIPELINE_SINGLE,
  bootstrapApp,
  importAndOpenPage,
} from './helpers/app'
import { drawStrokeOnCanvas } from './helpers/canvas'
import { prepareDetectAndOcr, runInpaint } from './helpers/pipeline'
import { selectors } from './helpers/selectors'

test.beforeEach(async ({ page }) => {
  await bootstrapApp(page)
})

test('brush and eraser interactions are available', async ({ page }) => {
  await importAndOpenPage(page, PIPELINE_SINGLE)
  await prepareDetectAndOcr(page)
  await runInpaint(page)

  await page.getByTestId(selectors.tools.brush).click()
  const brushCanvas = page.getByTestId(selectors.workspace.brushCanvas)
  await expect(brushCanvas).toBeVisible()
  await expect(page.getByTestId(selectors.workspace.brushDisplayCanvas)).toBeVisible()
  await expect(page.getByTestId(selectors.tools.brush)).toHaveAttribute(
    'data-active',
    'true',
  )

  await drawStrokeOnCanvas(page, brushCanvas, { x: 0.2, y: 0.25 }, { x: 0.65, y: 0.35 })

  await page.getByTestId(selectors.tools.eraser).click()
  await expect(page.getByTestId(selectors.tools.eraser)).toHaveAttribute(
    'data-active',
    'true',
  )
  await drawStrokeOnCanvas(page, brushCanvas, { x: 0.4, y: 0.3 }, { x: 0.55, y: 0.33 })
  await expect(page.getByTestId(selectors.workspace.canvas)).toBeVisible()
})
