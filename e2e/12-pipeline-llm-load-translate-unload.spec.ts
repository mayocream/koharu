import { expect, test } from './helpers/test'
import { PIPELINE_SINGLE, bootstrapApp, importAndOpenPage } from './helpers/app'
import {
  ensureLlmReady,
  ensureLlmUnloaded,
  generateTranslationForBlock,
  prepareDetectAndOcr,
} from './helpers/pipeline'
import { selectors } from './helpers/selectors'

test.beforeEach(async ({ page }) => {
  await bootstrapApp(page)
})

test('loads llm, translates a block, and unloads llm', async ({ page }) => {
  await importAndOpenPage(page, PIPELINE_SINGLE)
  await prepareDetectAndOcr(page)

  await ensureLlmReady(page)
  await expect(page.getByTestId(selectors.llm.loadToggle)).toHaveAttribute(
    'data-llm-ready',
    'true',
  )

  await page.getByTestId(selectors.panels.textBlockOcr(0)).fill('Hello world')
  await generateTranslationForBlock(page, 0)
  await expect(
    page.getByTestId(selectors.panels.textBlockTranslation(0)),
  ).not.toHaveValue('')

  await ensureLlmUnloaded(page)
  await expect(page.getByTestId(selectors.llm.loadToggle)).toHaveAttribute(
    'data-llm-ready',
    'false',
  )
})
