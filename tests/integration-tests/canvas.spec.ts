import type { Page } from '@playwright/test'

import { expect, test } from './fixtures'

interface ViewChanged {
  type: 'view_changed'
  zoom: number
  translation: [number, number]
  auto_fit: boolean
}

function nextViewChange(page: Page): Promise<ViewChanged> {
  return page.evaluate(
    () =>
      new Promise<ViewChanged>((resolve) => {
        const listener = (event: Event) => {
          const detail = (event as CustomEvent).detail as {
            type?: string
            payload?: ViewChanged
          }
          if (detail.type !== 'app' || detail.payload?.type !== 'view_changed') return
          window.removeEventListener('koharu:event', listener)
          resolve(detail.payload)
        }
        window.addEventListener('koharu:event', listener)
      }),
  )
}

test('pans, zooms, and restores the Rust canvas view', async ({ page }) => {
  const surface = page.getByLabel('Koharu canvas', { exact: true })
  test.skip(
    !(await surface.isVisible()),
    'open a project or start Koharu with a .khr fixture to exercise canvas input',
  )

  await expect(surface).toBeVisible()
  const bounds = await surface.boundingBox()
  if (!bounds) throw new Error('Koharu canvas has no visible bounds')

  const fitWindow = page.getByRole('button', { name: 'Fit Window', exact: true })
  const x = bounds.x + bounds.width / 2
  const y = bounds.y + bounds.height / 2

  try {
    const pannedPromise = nextViewChange(page)
    await page.mouse.move(x, y)
    await page.mouse.down({ button: 'middle' })
    await page.mouse.move(x + 80, y + 40, { steps: 8 })
    await page.mouse.up({ button: 'middle' })
    const panned = await pannedPromise
    expect(panned.auto_fit).toBe(false)

    const zoomedPromise = nextViewChange(page)
    await page.mouse.move(x, y)
    await page.mouse.wheel(0, -120)
    const zoomed = await zoomedPromise
    expect(zoomed.auto_fit).toBe(false)
    expect(zoomed.zoom).not.toBe(panned.zoom)
  } finally {
    const fittedPromise = nextViewChange(page)
    await fitWindow.click()
    const fitted = await fittedPromise
    expect(fitted.auto_fit).toBe(true)
  }
})
