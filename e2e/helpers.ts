import { expect, type Page } from '@playwright/test'
import path from 'node:path'

const FIXTURES_DIR = path.join(process.cwd(), 'e2e', 'fixtures')

export const FIXTURE_IMAGE_PATHS = [
  path.join(FIXTURES_DIR, '1.jpg'),
  path.join(FIXTURES_DIR, '10.jpg'),
  path.join(FIXTURES_DIR, '11.jpg'),
  path.join(FIXTURES_DIR, '12.jpg'),
  path.join(FIXTURES_DIR, '19.jpg'),
]

export async function enableFileInputFallback(page: Page) {
  await page.addInitScript(() => {
    try {
      // Force browser-fs-access to use input fallback that Playwright can control.
      delete (window as { showOpenFilePicker?: unknown }).showOpenFilePicker
    } catch {}
  })
}

export async function importImages(page: Page, filePaths: string[]) {
  const fileChooserPromise = page.waitForEvent('filechooser')
  await page.getByTestId('menu-file-trigger').click()
  await page.getByTestId('menu-file-open').click()
  const fileChooser = await fileChooserPromise
  await fileChooser.setFiles(filePaths)
}

export async function openFirstDocument(page: Page) {
  const firstPageButton = page.getByTestId('navigator-page-0')
  await expect(firstPageButton).toBeVisible()
  await firstPageButton.click()
}

export async function getWorkspaceViewport(page: Page) {
  const viewport = page.getByTestId('workspace-viewport')
  await expect(viewport).toBeVisible()
  return viewport
}

export async function readZoomPercent(page: Page) {
  const value = await page
    .getByTestId('zoom-slider')
    .getByRole('slider')
    .first()
    .getAttribute('aria-valuenow')
  const parsed = Number(value)
  if (!Number.isFinite(parsed)) {
    throw new Error(`Unable to parse zoom slider value: ${String(value)}`)
  }
  return parsed
}

export async function waitForNavigatorPageCount(
  page: Page,
  expectedCount: number,
) {
  await expect
    .poll(async () => {
      const value = await page
        .getByTestId('navigator-panel')
        .getAttribute('data-total-pages')
      const parsed = Number(value)
      return Number.isFinite(parsed) ? parsed : 0
    })
    .toBe(expectedCount)
}
