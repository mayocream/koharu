import { expect, test } from './helpers/test'
import {
  PIPELINE_SINGLE,
  bootstrapApp,
  importAndOpenPage,
  waitForLayerHasContent,
} from './helpers/app'
import { readImageSrc, waitForImageSrcChange } from './helpers/canvas'
import { prepareDetectAndOcr, runInpaint, runRender } from './helpers/pipeline'
import { selectors } from './helpers/selectors'

test.beforeEach(async ({ page }) => {
  await bootstrapApp(page)
})

test('changes style controls and re-renders', async ({ page }) => {
  await importAndOpenPage(page, PIPELINE_SINGLE)
  await prepareDetectAndOcr(page)

  await page.getByTestId(selectors.panels.renderControlsTrigger).click()
  await expect(page.getByTestId(selectors.panels.renderControlsPopover)).toBeVisible()

  await page.getByTestId(selectors.panels.renderFontSelect).click()
  await expect(page.getByTestId(selectors.panels.renderFontOption(1))).toBeVisible()
  await page.getByTestId(selectors.panels.renderFontOption(1)).click()

  const swatch = page.getByTestId(selectors.panels.renderColorSwatch)
  const colorBefore = await swatch.evaluate((node) => {
    if (!(node instanceof HTMLElement)) return ''
    return node.style.backgroundColor
  })

  await page.getByTestId(selectors.panels.renderColorTrigger).click()
  const colorPicker = page.getByTestId(selectors.panels.renderColorPicker)
  await expect(colorPicker).toBeVisible()
  const pickerBox = await colorPicker.boundingBox()
  if (!pickerBox) {
    throw new Error('render color picker is not visible')
  }
  await page.mouse.click(pickerBox.x + pickerBox.width * 0.15, pickerBox.y + pickerBox.height * 0.2)
  await expect
    .poll(
      async () =>
        swatch.evaluate((node) => {
          if (!(node instanceof HTMLElement)) return ''
          return node.style.backgroundColor
        }),
      { timeout: 30_000 },
    )
    .not.toBe(colorBefore)

  await runInpaint(page)
  await runRender(page)
  await waitForLayerHasContent(page, 'rendered', true)

  const renderedImage = page.getByTestId(selectors.workspace.renderedImage)
  await expect(renderedImage).toBeVisible()
  const firstRenderedSrc = await readImageSrc(renderedImage)

  await page.getByTestId(selectors.panels.renderControlsTrigger).click()
  await expect(page.getByTestId(selectors.panels.renderControlsPopover)).toBeVisible()
  await page.getByTestId(selectors.panels.renderEffectSelect).click()
  await expect(page.getByTestId(selectors.panels.renderEffectOption(1))).toBeVisible()
  await page.getByTestId(selectors.panels.renderEffectOption(1)).click()

  await runRender(page)
  await waitForImageSrcChange(renderedImage, firstRenderedSrc, 180_000)
})
