import {
  expect,
  test as base,
  type ConsoleMessage,
  type TestInfo,
} from '@playwright/test'

function formatLocation(message: ConsoleMessage) {
  const location = message.location()
  if (!location.url) return ''
  return ` (${location.url}:${location.lineNumber ?? 0}:${location.columnNumber ?? 0})`
}

function normalizeConsoleError(message: ConsoleMessage) {
  return `[console.${message.type()}] ${message.text()}${formatLocation(message)}`
}

export const test = base.extend({
  page: async ({ page }, use, testInfo: TestInfo) => {
    const errors: string[] = []

    page.on('console', (message) => {
      if (message.type() !== 'error') return
      errors.push(normalizeConsoleError(message))
    })

    page.on('pageerror', (error) => {
      errors.push(`[pageerror] ${error.name}: ${error.message}`)
    })

    await use(page)

    if (errors.length > 0) {
      const body = errors.join('\n')
      await testInfo.attach('browser-errors', {
        body,
        contentType: 'text/plain',
      })
      throw new Error(`Browser console errors detected:\n${body}`)
    }
  },
})

export { expect }
