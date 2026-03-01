import path from 'node:path'
import { expect, type Locator, type Page } from '@playwright/test'
import { selectors, type LayerId } from './selectors'

const FIXTURES_DIR = path.join(process.cwd(), 'e2e', 'fixtures')

export const FIXTURE_IMAGE_PATHS = [
  path.join(FIXTURES_DIR, '1.jpg'),
  path.join(FIXTURES_DIR, '10.jpg'),
  path.join(FIXTURES_DIR, '11.jpg'),
  path.join(FIXTURES_DIR, '12.jpg'),
  path.join(FIXTURES_DIR, '13.jpg'),
  path.join(FIXTURES_DIR, '19.jpg'),
  path.join(FIXTURES_DIR, '20.jpg'),
  path.join(FIXTURES_DIR, '21.jpg'),
]

export const SMOKE_SET = FIXTURE_IMAGE_PATHS.slice(0, 5)
export const PIPELINE_SINGLE = [FIXTURE_IMAGE_PATHS[0]]
export const PROCESS_ALL_SET = FIXTURE_IMAGE_PATHS

const APP_STORAGE_KEYS = ['koharu-rq-v1']
const APP_STORAGE_PREFIXES = ['react-resizable-panels', 'koharu-main-layout']

export async function setupDeterministicPage(page: Page) {
  await page.addInitScript(
    ({ keys, prefixes }) => {
      try {
        localStorage.clear()
        sessionStorage.clear()
      } catch {}

      try {
        for (const key of keys) {
          localStorage.removeItem(key)
        }

        const allKeys: string[] = []
        for (let i = 0; i < localStorage.length; i += 1) {
          const key = localStorage.key(i)
          if (key) allKeys.push(key)
        }

        for (const key of allKeys) {
          if (prefixes.some((prefix) => key.startsWith(prefix))) {
            localStorage.removeItem(key)
          }
        }

        localStorage.setItem('i18nextLng', 'en-US')
      } catch {}

      try {
        delete (window as { showOpenFilePicker?: unknown }).showOpenFilePicker
      } catch {}

      try {
        delete (window as { showSaveFilePicker?: unknown }).showSaveFilePicker
      } catch {}
    },
    {
      keys: APP_STORAGE_KEYS,
      prefixes: APP_STORAGE_PREFIXES,
    },
  )
}

export async function openApp(page: Page) {
  await page.goto('/')
  await expect(page.getByTestId(selectors.menu.fileTrigger)).toBeVisible()
}

export async function bootstrapApp(page: Page) {
  await setupDeterministicPage(page)
  await openApp(page)
}

export async function importImages(page: Page, filePaths: string[]) {
  const fileChooserPromise = page.waitForEvent('filechooser')
  await page.getByTestId(selectors.menu.fileTrigger).click()
  await page.getByTestId(selectors.menu.fileOpen).click()
  const fileChooser = await fileChooserPromise
  await fileChooser.setFiles(filePaths)
}

export async function waitForNavigatorPageCount(
  page: Page,
  expectedCount: number,
  timeout = 60_000,
) {
  await expect
    .poll(
      async () => {
        const value = await page
          .getByTestId(selectors.navigator.panel)
          .getAttribute('data-total-pages')
        const parsed = Number(value)
        return Number.isFinite(parsed) ? parsed : 0
      },
      {
        timeout,
      },
    )
    .toBe(expectedCount)
}

export async function openNavigatorPage(page: Page, index: number) {
  const button = page.getByTestId(selectors.navigator.page(index))
  await expect(button).toBeVisible()
  await button.click()
}

export async function getWorkspaceViewport(page: Page) {
  const viewport = page.getByTestId(selectors.workspace.viewport)
  await expect(viewport).toBeVisible()
  return viewport
}

export async function getWorkspaceCanvas(page: Page) {
  const canvas = page.getByTestId(selectors.workspace.canvas)
  await expect(canvas).toBeVisible()
  return canvas
}

export async function waitForWorkspaceImage(page: Page) {
  const canvas = await getWorkspaceCanvas(page)
  await expect(canvas.locator('img').first()).toBeVisible()
}

export async function importAndOpenPage(
  page: Page,
  filePaths: string[],
  pageIndex = 0,
) {
  await importImages(page, filePaths)
  await waitForNavigatorPageCount(page, filePaths.length)
  await openNavigatorPage(page, pageIndex)
  await waitForWorkspaceImage(page)
}

export async function openMenuItem(
  page: Page,
  triggerTestId: string,
  itemTestId: string,
) {
  await page.getByTestId(triggerTestId).click()
  await page.getByTestId(itemTestId).click()
}

export function getLayerLocator(page: Page, layerId: LayerId): Locator {
  return page.getByTestId(selectors.layers[layerId])
}

export async function waitForLayerHasContent(
  page: Page,
  layerId: LayerId,
  hasContent = true,
  timeout = 45_000,
) {
  const layer = getLayerLocator(page, layerId)
  await expect(layer).toHaveAttribute(
    'data-has-content',
    hasContent ? 'true' : 'false',
    { timeout },
  )
}

export async function waitForLayerVisible(
  page: Page,
  layerId: LayerId,
  visible = true,
  timeout = 30_000,
) {
  const layer = getLayerLocator(page, layerId)
  await expect(layer).toHaveAttribute('data-visible', visible ? 'true' : 'false', {
    timeout,
  })
}

export async function readTextBlocksCount(page: Page) {
  const value = await page
    .getByTestId(selectors.panels.textBlocksCount)
    .getAttribute('data-count')
  const parsed = Number(value)
  return Number.isFinite(parsed) ? parsed : 0
}
