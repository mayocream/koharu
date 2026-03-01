import { expect, test } from './helpers/test'
import {
  SMOKE_SET,
  bootstrapApp,
  importAndOpenPage,
  openNavigatorPage,
  readTextBlocksCount,
  waitForLayerHasContent,
} from './helpers/app'
import { drawStrokeOnCanvas } from './helpers/canvas'
import { prepareDetectAndOcr } from './helpers/pipeline'
import { selectors } from './helpers/selectors'

test.beforeEach(async ({ page }) => {
  await bootstrapApp(page)
})

test('drafts a block and edits OCR/translation fields', async ({
  page,
}) => {
  await importAndOpenPage(page, SMOKE_SET.slice(0, 2))
  await prepareDetectAndOcr(page)

  const countBefore = await readTextBlocksCount(page)

  await page.getByTestId(selectors.tools.block).click()
  await drawStrokeOnCanvas(page, page.getByTestId(selectors.workspace.canvas), {
    x: 0.15,
    y: 0.15,
  }, {
    x: 0.32,
    y: 0.22,
  })

  await expect
    .poll(async () => readTextBlocksCount(page), { timeout: 45_000 })
    .toBe(countBefore + 1)

  const ocrValue = 'E2E OCR Edited'
  const translationValue = 'E2E Translation Edited'

  await page.getByTestId(selectors.panels.textBlockCard(0)).click()
  await expect(page.getByTestId(selectors.panels.textBlockOcr(0))).toBeVisible()
  await page.getByTestId(selectors.panels.textBlockOcr(0)).fill(ocrValue)
  await page
    .getByTestId(selectors.panels.textBlockTranslation(0))
    .fill(translationValue)
  await expect(page.getByTestId(selectors.panels.textBlockOcr(0))).toHaveValue(
    ocrValue,
  )
  await expect(
    page.getByTestId(selectors.panels.textBlockTranslation(0)),
  ).toHaveValue(translationValue)
  await page.getByTestId(selectors.toolbar.inpaint).click()
  await waitForLayerHasContent(page, 'inpainted', true)

  await openNavigatorPage(page, 1)
  await openNavigatorPage(page, 0)
  await expect(page.getByTestId(selectors.workspace.canvas)).toBeVisible()
})
