import { expect, test } from '@playwright/test'
import {
  enableFileInputFallback,
  FIXTURE_IMAGE_PATHS,
  getWorkspaceViewport,
  importImages,
  waitForNavigatorPageCount,
} from './helpers'

test.beforeEach(async ({ page }) => {
  await enableFileInputFallback(page)
})

test('loads images via File > Open', async ({ page }) => {
  await page.goto('/')
  await importImages(page, FIXTURE_IMAGE_PATHS)
  await waitForNavigatorPageCount(page, FIXTURE_IMAGE_PATHS.length)

  await page.getByTestId('navigator-page-0').click()

  const viewport = await getWorkspaceViewport(page)
  await expect(page.getByTestId('workspace-canvas')).toBeVisible()
  await expect(viewport.locator('img[draggable="false"]').first()).toBeVisible()
})
