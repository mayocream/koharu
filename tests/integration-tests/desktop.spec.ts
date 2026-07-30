import { mkdir, stat } from 'node:fs/promises'
import path from 'node:path'

import { expect, test } from './fixtures'
import { saveProjectAs } from './native-dialog'

test.afterEach(async ({ page }) => {
  await page.keyboard.press('Escape')
  const restore = page.getByRole('button', { name: 'Restore', exact: true })
  if (await restore.isVisible()) await restore.click()
})

test('exposes the native bridge and persistent desktop controls', async ({ page }) => {
  await expect(page).toHaveTitle('Koharu')
  await expect(page.getByRole('menuitem', { name: 'File', exact: true })).toBeVisible()
  await expect(page.getByRole('menuitem', { name: 'Edit', exact: true })).toBeVisible()
  await expect(page.getByRole('menuitem', { name: 'Process', exact: true })).toBeVisible()
  await expect(page.getByRole('menuitem', { name: 'View', exact: true })).toBeVisible()
  await expect(page.getByRole('button', { name: 'Minimize', exact: true })).toBeVisible()
  await expect(page.getByRole('button', { name: 'Maximize', exact: true })).toBeVisible()
  await expect(page.getByRole('button', { name: 'Close', exact: true })).toBeVisible()

  await expect
    .poll(() =>
      page.evaluate(() => {
        const bridge = (window as Window & { koharu?: Record<string, unknown> }).koharu
        return {
          send: typeof bridge?.send,
          listen: typeof bridge?.listen,
        }
      }),
    )
    .toEqual({ send: 'function', listen: 'function' })
})

test('creates a project through the native Save As dialog', async ({ page }, testInfo) => {
  test.skip(process.platform !== 'win32', 'WebView2 desktop tests require Windows')

  const canvas = page.getByLabel('Koharu canvas', { exact: true })
  test.skip(await canvas.isVisible(), 'a project is already open')

  const projectPath = testInfo.outputPath('New-Project.khr')
  await mkdir(path.dirname(projectPath), { recursive: true })
  const saveProject = saveProjectAs(projectPath)

  try {
    await page.getByRole('button', { name: 'New Project', exact: true }).click()
    await saveProject

    await expect(canvas).toBeVisible()
    expect((await stat(projectPath)).size).toBeGreaterThan(0)
  } finally {
    await saveProject.catch(() => {})
    if (await canvas.isVisible()) {
      await page.getByRole('menuitem', { name: 'File', exact: true }).click()
      await page.getByRole('menuitem', { name: 'Close Project', exact: true }).click()
      await expect(page.getByRole('button', { name: 'New Project', exact: true })).toBeVisible()
    }
  }
})

test('opens the settings dialog through the native menu and dismisses it', async ({ page }) => {
  await page.getByRole('menuitem', { name: 'File', exact: true }).click()
  const settings = page.getByRole('menuitem', { name: /^Settings/ })
  await expect(settings).toBeVisible()
  await settings.click()

  const dialog = page.getByRole('dialog', { name: 'Settings' })
  await expect(dialog).toBeVisible()
  await page.keyboard.press('Escape')
  await expect(dialog).toBeHidden()
})

test('maximizes and restores the native window', async ({ page }) => {
  const maximize = page.getByRole('button', { name: 'Maximize', exact: true })
  const restore = page.getByRole('button', { name: 'Restore', exact: true })
  await expect(maximize).toBeVisible()

  const initialSize = await page.evaluate(() => ({
    width: window.innerWidth,
    height: window.innerHeight,
  }))
  try {
    await maximize.click()
    await expect(restore).toBeVisible()
    await expect
      .poll(() => page.evaluate(() => ({ width: window.innerWidth, height: window.innerHeight })))
      .not.toEqual(initialSize)
  } finally {
    if (await restore.isVisible()) await restore.click()
  }

  await expect(maximize).toBeVisible()
  await expect
    .poll(() => page.evaluate(() => ({ width: window.innerWidth, height: window.innerHeight })))
    .toEqual(initialSize)
})

test('shows project entry points when no project is open', async ({ page }) => {
  test.skip(
    await page.getByLabel('Koharu canvas', { exact: true }).isVisible(),
    'a project is open',
  )

  await expect(page.getByRole('button', { name: 'New Project', exact: true })).toBeVisible()
  await expect(page.getByRole('button', { name: 'Open Project', exact: true })).toBeVisible()
})
