import { expect, type Page } from '@playwright/test'
import {
  openMenuItem,
  readTextBlocksCount,
  waitForLayerHasContent,
} from './app'
import { selectors } from './selectors'

export async function runDetect(page: Page, timeout = 45_000) {
  await page.getByTestId(selectors.toolbar.detect).click()
  await waitForLayerHasContent(page, 'mask', true, timeout)
}

export async function runOcr(page: Page, timeout = 45_000) {
  await page.getByTestId(selectors.toolbar.ocr).click()
  await expect
    .poll(async () => readTextBlocksCount(page), { timeout })
    .toBeGreaterThan(0)
  const firstCard = page.getByTestId(selectors.panels.textBlockCard(0))
  await expect(firstCard).toBeVisible({ timeout })
  await firstCard.click()
  await expect(page.getByTestId(selectors.panels.textBlockOcr(0))).toBeVisible({
    timeout,
  })
}

export async function runInpaint(page: Page, timeout = 45_000) {
  await page.getByTestId(selectors.toolbar.inpaint).click()
  await waitForLayerHasContent(page, 'inpainted', true, timeout)
}

export async function runRender(page: Page, timeout = 45_000) {
  await page.getByTestId(selectors.toolbar.render).click()
  await waitForLayerHasContent(page, 'rendered', true, timeout)
}

export async function prepareDetectAndOcr(page: Page) {
  await runDetect(page)
  await runOcr(page)
}

export async function openLlmPopover(page: Page) {
  await page.getByTestId(selectors.llm.trigger).click()
  await expect(page.getByTestId(selectors.llm.popover)).toBeVisible()
}

async function assertLlmModelOptions(page: Page) {
  await page.getByTestId(selectors.llm.modelSelect).click()
  const firstOption = page.getByTestId(selectors.llm.modelOption(0))
  await expect(firstOption).toBeVisible({ timeout: 60_000 })
  await firstOption.click()
}

async function assertLlmLanguageOptions(page: Page) {
  const select = page.getByTestId(selectors.llm.languageSelect)
  if ((await select.count()) === 0) return
  await select.click()
  const firstOption = page.getByTestId(selectors.llm.languageOption(0))
  await expect(firstOption).toBeVisible({ timeout: 30_000 })
  await firstOption.click()
}

export async function ensureLlmReady(page: Page, timeout = 60_000) {
  await openLlmPopover(page)
  await assertLlmModelOptions(page)
  await assertLlmLanguageOptions(page)

  const loadToggle = page.getByTestId(selectors.llm.loadToggle)
  const readyBefore = await loadToggle.getAttribute('data-llm-ready')
  if (readyBefore === 'true') return

  await loadToggle.click()
  await expect
    .poll(async () => loadToggle.getAttribute('data-llm-ready'), { timeout })
    .toBe('true')
}

export async function ensureLlmUnloaded(page: Page, timeout = 30_000) {
  await openLlmPopover(page)
  const loadToggle = page.getByTestId(selectors.llm.loadToggle)
  const readyBefore = await loadToggle.getAttribute('data-llm-ready')
  if (readyBefore !== 'true') return

  await loadToggle.click()
  await expect
    .poll(async () => loadToggle.getAttribute('data-llm-ready'), { timeout })
    .toBe('false')
}

export async function generateTranslationForBlock(
  page: Page,
  blockIndex = 0,
  timeout = 45_000,
) {
  const field = page.getByTestId(selectors.panels.textBlockTranslation(blockIndex))
  const previous = await field.inputValue()
  await page.getByTestId(selectors.panels.textBlockGenerate(blockIndex)).click()

  await expect
    .poll(async () => field.inputValue(), { timeout })
    .not.toEqual(previous)
}

export async function startProcessCurrent(page: Page) {
  await openMenuItem(
    page,
    selectors.menu.processTrigger,
    selectors.menu.processCurrent,
  )
}

export async function startProcessAll(page: Page) {
  await openMenuItem(page, selectors.menu.processTrigger, selectors.menu.processAll)
}

export async function waitForOperationStart(
  page: Page,
  operationType: 'process-current' | 'process-all' | 'llm-load',
  timeout = 45_000,
) {
  const card = page.getByTestId(selectors.operations.card)
  await expect(card).toBeVisible({ timeout })
  await expect(card).toHaveAttribute('data-operation-type', operationType, {
    timeout,
  })
}

export async function waitForOperationFinish(page: Page, timeout = 90_000) {
  await expect(page.getByTestId(selectors.operations.card)).toBeHidden({
    timeout,
  })
}

export async function waitForOperationProgressAdvance(
  page: Page,
  timeout = 45_000,
) {
  const card = page.getByTestId(selectors.operations.card)
  const initialCurrent = Number((await card.getAttribute('data-current')) ?? '0')
  await expect
    .poll(async () => {
      const raw = await card.getAttribute('data-current')
      const value = Number(raw ?? '0')
      return Number.isFinite(value) ? value : 0
    }, { timeout })
    .toBeGreaterThan(initialCurrent)
}
