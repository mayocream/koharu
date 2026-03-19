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

test('drafts a block and edits OCR/translation fields', async ({ page }) => {
  await importAndOpenPage(page, SMOKE_SET.slice(0, 2))
  await prepareDetectAndOcr(page)

  const countBefore = await readTextBlocksCount(page)

  await page.getByTestId(selectors.tools.block).click()
  await drawStrokeOnCanvas(
    page,
    page.getByTestId(selectors.workspace.canvas),
    {
      x: 0.15,
      y: 0.15,
    },
    {
      x: 0.32,
      y: 0.22,
    },
  )

  await expect
    .poll(async () => readTextBlocksCount(page), { timeout: 45_000 })
    .toBe(countBefore + 1)

  const ocrValue = 'E2E OCR Edited'
  const translationValue = 'E2E Translation Edited'
  const ocrField = page.getByTestId(selectors.panels.textBlockOcr(0))
  const translationField = page.getByTestId(
    selectors.panels.textBlockTranslation(0),
  )

  await page.getByTestId(selectors.panels.textBlockCard(0)).click()
  await expect(ocrField).toBeVisible()
  await ocrField.fill(ocrValue)
  await translationField.fill(translationValue)
  await expect(ocrField).toHaveValue(ocrValue)
  await expect(translationField).toHaveValue(translationValue)

  await ocrField.press('Home')
  await ocrField.type('X')
  await expect
    .poll(
      () =>
        ocrField.evaluate((element: HTMLTextAreaElement) => ({
          value: element.value,
          start: element.selectionStart,
          end: element.selectionEnd,
        })),
      { timeout: 5_000 },
    )
    .toEqual({
      value: `X${ocrValue}`,
      start: 1,
      end: 1,
    })

  await translationField.press('Home')
  await translationField.type('Y')
  await expect
    .poll(
      () =>
        translationField.evaluate((element: HTMLTextAreaElement) => ({
          value: element.value,
          start: element.selectionStart,
          end: element.selectionEnd,
        })),
      { timeout: 5_000 },
    )
    .toEqual({
      value: `Y${translationValue}`,
      start: 1,
      end: 1,
    })

  await page.getByTestId(selectors.toolbar.inpaint).click()
  await waitForLayerHasContent(page, 'inpainted', true)

  await openNavigatorPage(page, 1)
  await openNavigatorPage(page, 0)
  await expect(page.getByTestId(selectors.workspace.canvas)).toBeVisible()
})

test('deletes a text block from the panel and updates count', async ({ page }) => {
  await importAndOpenPage(page, SMOKE_SET.slice(0, 2))
  await prepareDetectAndOcr(page)

  const countBefore = await readTextBlocksCount(page)
  expect(countBefore).toBeGreaterThan(0)

  const cards = page.locator('[data-testid^="textblock-card-"]')
  await expect(cards).toHaveCount(countBefore)

  await page.getByTestId(selectors.panels.textBlockCard(0)).click()
  await page.getByTestId(selectors.panels.textBlockDelete(0)).click()

  await expect
    .poll(async () => readTextBlocksCount(page), { timeout: 15_000 })
    .toBe(countBefore - 1)
  await expect(cards).toHaveCount(countBefore - 1)
})
