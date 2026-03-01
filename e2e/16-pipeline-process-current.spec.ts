import { test } from './helpers/test'
import {
  PIPELINE_SINGLE,
  bootstrapApp,
  importAndOpenPage,
  waitForLayerHasContent,
} from './helpers/app'
import {
  ensureLlmReady,
  startProcessCurrent,
  waitForOperationFinish,
  waitForOperationProgressAdvance,
  waitForOperationStart,
} from './helpers/pipeline'

test.beforeEach(async ({ page }) => {
  await bootstrapApp(page)
})

test('process current completes and renders the page', async ({ page }) => {
  await importAndOpenPage(page, PIPELINE_SINGLE)
  await ensureLlmReady(page)

  await startProcessCurrent(page)
  await waitForOperationStart(page, 'process-current')
  await waitForOperationProgressAdvance(page)
  await waitForOperationFinish(page, 360_000)

  await waitForLayerHasContent(page, 'rendered', true)
})
