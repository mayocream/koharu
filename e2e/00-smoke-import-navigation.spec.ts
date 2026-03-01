import { expect, test } from './helpers/test'
import {
  SMOKE_SET,
  bootstrapApp,
  importAndOpenPage,
  openNavigatorPage,
  waitForNavigatorPageCount,
  waitForWorkspaceImage,
} from './helpers/app'
import { selectors } from './helpers/selectors'

test.beforeEach(async ({ page }) => {
  await bootstrapApp(page)
})

test('imports images and navigates pages with visible thumbnails', async ({
  page,
}) => {
  await importAndOpenPage(page, SMOKE_SET)
  await waitForNavigatorPageCount(page, SMOKE_SET.length)

  const lastIndex = SMOKE_SET.length - 1
  await openNavigatorPage(page, lastIndex)
  await waitForWorkspaceImage(page)

  await openNavigatorPage(page, 0)
  await waitForWorkspaceImage(page)

  const panel = page.getByTestId(selectors.navigator.panel)
  const scrollViewport = panel.locator('[data-slot="scroll-area-viewport"]').first()
  await scrollViewport.evaluate((node) => {
    node.scrollTop = node.scrollHeight
  })

  const lastPreview = page.getByTestId(selectors.navigator.page(lastIndex))
  await expect(lastPreview).toBeVisible()
  await expect(lastPreview.locator('img').first()).toBeVisible()
})
