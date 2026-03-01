import { test } from './helpers/test'
import {
  SMOKE_SET,
  bootstrapApp,
  importAndOpenPage,
  openNavigatorPage,
  waitForLayerHasContent,
} from './helpers/app'
import {
  ensureLlmReady,
  startProcessAll,
  waitForOperationFinish,
  waitForOperationProgressAdvance,
  waitForOperationStart,
} from './helpers/pipeline'

test.beforeEach(async ({ page }) => {
  await bootstrapApp(page)
})

test('process all completes and renders every imported page', async ({ page }) => {
  const files = SMOKE_SET.slice(0, 3)
  await importAndOpenPage(page, files)
  await ensureLlmReady(page)

  await startProcessAll(page)
  await waitForOperationStart(page, 'process-all')
  await waitForOperationProgressAdvance(page)
  await waitForOperationFinish(page, 420_000)

  for (let index = 0; index < files.length; index += 1) {
    await openNavigatorPage(page, index)
    await waitForLayerHasContent(page, 'rendered', true, 120_000)
  }
})
