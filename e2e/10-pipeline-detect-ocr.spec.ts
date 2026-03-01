import { expect, test } from './helpers/test'
import { PIPELINE_SINGLE, bootstrapApp, importAndOpenPage } from './helpers/app'
import { runDetect, runOcr } from './helpers/pipeline'
import { selectors } from './helpers/selectors'

test.beforeEach(async ({ page }) => {
  await bootstrapApp(page)
})

test('detect and ocr populate mask and text blocks', async ({ page }) => {
  await importAndOpenPage(page, PIPELINE_SINGLE)

  await runDetect(page)
  await runOcr(page)

  await expect(page.getByTestId(selectors.layers.mask)).toHaveAttribute(
    'data-has-content',
    'true',
  )
  await expect
    .poll(async () => {
      const count = await page
        .getByTestId(selectors.panels.textBlocksCount)
        .getAttribute('data-count')
      return Number(count ?? '0')
    })
    .toBeGreaterThan(0)
  await expect(page.getByTestId(selectors.panels.textBlockOcr(0))).toBeVisible()
})
