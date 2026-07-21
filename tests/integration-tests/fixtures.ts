import { readFile } from 'node:fs/promises'
import path from 'node:path'

import { expect, test as base, type Browser, type Page } from '@playwright/test'

interface Desktop {
  browser: Browser
  page: Page
}

interface WorkerFixtures {
  desktop: Desktop
}

const workspaceRoot = path.resolve(__dirname, '../..')
const url = 'http://localhost:3000/'
const portFile = path.join(
  workspaceRoot,
  'target',
  'release-with-debug',
  'koharu.exe.WebView2',
  'EBWebView',
  'DevToolsActivePort',
)

async function waitForDevToolsEndpoint(): Promise<string> {
  const deadline = Date.now() + 15_000
  let lastError: unknown

  while (Date.now() < deadline) {
    try {
      const [line] = (await readFile(portFile, 'utf8')).split(/\r?\n/)
      const port = Number.parseInt(line, 10)
      if (Number.isInteger(port) && port > 0 && port <= 65_535) {
        return `http://127.0.0.1:${port}`
      }
      lastError = new Error(`invalid DevTools port ${JSON.stringify(line)}`)
    } catch (error) {
      lastError = error
    }
    await new Promise((resolve) => setTimeout(resolve, 100))
  }

  throw new Error(
    [
      `Koharu's WebView2 DevTools endpoint was not found at ${portFile}.`,
      'Start `bun run dev` and wait for the desktop window before running the integration tests.',
      lastError instanceof Error ? `Last error: ${lastError.message}` : undefined,
    ]
      .filter(Boolean)
      .join(' '),
  )
}

async function waitForPage(browser: Browser): Promise<Page> {
  const deadline = Date.now() + 15_000

  while (Date.now() < deadline) {
    for (const context of browser.contexts()) {
      for (const candidate of context.pages()) {
        if (candidate.url().startsWith(url)) return candidate
        if ((await candidate.title().catch(() => '')) === 'Koharu') return candidate
      }
    }
    await new Promise((resolve) => setTimeout(resolve, 100))
  }

  throw new Error(`Koharu's WebView2 page was not found at ${url}`)
}

export const test = base.extend<{}, WorkerFixtures>({
  desktop: [
    async ({ playwright }, use) => {
      const endpoint = await waitForDevToolsEndpoint()
      const browser = await playwright.chromium.connectOverCDP(endpoint, {
        isLocal: true,
        noDefaults: true,
        timeout: 15_000,
      })
      try {
        const page = await waitForPage(browser)
        await page.waitForFunction(
          () =>
            typeof (window as Window & { koharu?: { send?: unknown } }).koharu?.send === 'function',
        )
        await use({ browser, page })
      } finally {
        await browser.close()
      }
    },
    { scope: 'worker' },
  ],
  page: async ({ desktop }, use) => {
    await use(desktop.page)
  },
})

export { expect }
