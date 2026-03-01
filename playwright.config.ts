import { defineConfig } from '@playwright/test'

export default defineConfig({
  testDir: './e2e',
  fullyParallel: false,
  workers: 1,
  timeout: 120_000,
  expect: {
    timeout: 5_000,
  },
  use: {
    baseURL: 'http://localhost:3000',
    headless: true,
    acceptDownloads: true,
    trace: 'retain-on-failure',
    screenshot: 'only-on-failure',
  },
  webServer: {
    command: 'bun run dev -- --headless',
    url: 'http://localhost:3000',
    reuseExistingServer: true,
    timeout: 90_000,
    wait: {
      stdout: /Running (.*) --headless/,
    },
  },
})
