import { expect, test } from './helpers/test'
import { stat } from 'node:fs/promises'
import {
  PIPELINE_SINGLE,
  bootstrapApp,
  importAndOpenPage,
  openMenuItem,
} from './helpers/app'
import { prepareDetectAndOcr, runInpaint, runRender } from './helpers/pipeline'
import { selectors } from './helpers/selectors'

test.beforeEach(async ({ page }) => {
  await bootstrapApp(page)
})

test('exports rendered image via file menu', async ({ page }) => {
  await importAndOpenPage(page, PIPELINE_SINGLE)
  await prepareDetectAndOcr(page)
  await runInpaint(page)
  await runRender(page)

  const downloadPromise = page.waitForEvent('download')
  await openMenuItem(page, selectors.menu.fileTrigger, selectors.menu.fileExport)
  const download = await downloadPromise

  const suggested = download.suggestedFilename()
  expect(suggested).toMatch(/_koharu\.[A-Za-z0-9]+$/)

  const downloadPath = await download.path()
  expect(downloadPath).toBeTruthy()
  if (!downloadPath) {
    throw new Error('Download path was empty')
  }

  const info = await stat(downloadPath)
  expect(info.size).toBeGreaterThan(0)
})
