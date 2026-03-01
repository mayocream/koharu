import { expect, test } from './helpers/test'
import {
  PROCESS_ALL_SET,
  bootstrapApp,
  getLayerLocator,
  importAndOpenPage,
  openNavigatorPage,
} from './helpers/app'
import {
  ensureLlmReady,
  startProcessAll,
  waitForOperationFinish,
  waitForOperationStart,
} from './helpers/pipeline'
import { selectors } from './helpers/selectors'

test.beforeEach(async ({ page }) => {
  await bootstrapApp(page)
})

test('process all can be cancelled before all pages are rendered', async ({
  page,
}) => {
  await importAndOpenPage(page, PROCESS_ALL_SET)
  await ensureLlmReady(page)

  await startProcessAll(page)
  await waitForOperationStart(page, 'process-all')

  const cancelButton = page.getByTestId(selectors.operations.cancel)
  await expect(cancelButton).toBeVisible()
  await cancelButton.click()

  const operationCard = page.getByTestId(selectors.operations.card)
  await expect(operationCard).toHaveAttribute('data-cancel-requested', 'true')
  await waitForOperationFinish(page, 300_000)

  let renderedPages = 0
  for (let index = 0; index < PROCESS_ALL_SET.length; index += 1) {
    await openNavigatorPage(page, index)
    const hasRendered = await getLayerLocator(page, 'rendered').getAttribute(
      'data-has-content',
    )
    if (hasRendered === 'true') renderedPages += 1
  }

  expect(renderedPages).toBeLessThan(PROCESS_ALL_SET.length)
})
