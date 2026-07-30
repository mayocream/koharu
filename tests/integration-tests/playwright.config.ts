import path from 'node:path'

import { defineConfig } from '@playwright/test'

export default defineConfig({
  testDir: '.',
  testMatch: '**/*.spec.ts',
  outputDir: path.resolve(__dirname, 'test-results'),
  fullyParallel: false,
  workers: 1,
  forbidOnly: Boolean(process.env.CI),
  retries: 0,
  timeout: 15_000,
  expect: {
    timeout: 5_000,
  },
  reporter: 'list',
})
