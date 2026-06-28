import { describe, expect, it } from 'vitest'

import {
  CHAPTER_MAX_BLOCKS_DEFAULT,
  CHAPTER_TOKEN_BUDGET_DEFAULT,
  chapterTranslationPipelineOptions,
  clampChapterTranslationMaxBlocks,
  clampChapterTranslationTokenBudget,
} from '@/lib/pipeline/chapterContextSettings'

describe('chapterContextSettings', () => {
  it('clamps token budget to allowed range', () => {
    expect(clampChapterTranslationTokenBudget(100)).toBe(256)
    expect(clampChapterTranslationTokenBudget(9000)).toBe(8192)
    expect(clampChapterTranslationTokenBudget(4096)).toBe(4096)
  })

  it('clamps max blocks to allowed range', () => {
    expect(clampChapterTranslationMaxBlocks(0)).toBe(1)
    expect(clampChapterTranslationMaxBlocks(500)).toBe(200)
    expect(clampChapterTranslationMaxBlocks(100)).toBe(100)
  })

  it('includes chapter options only when enabled with translator', () => {
    expect(
      chapterTranslationPipelineOptions({
        chapterContextTranslation: false,
        chapterTranslationTokenBudget: CHAPTER_TOKEN_BUDGET_DEFAULT,
        chapterTranslationMaxBlocks: CHAPTER_MAX_BLOCKS_DEFAULT,
      }),
    ).toEqual({})

    expect(
      chapterTranslationPipelineOptions({
        chapterContextTranslation: true,
        chapterTranslationTokenBudget: 2048,
        chapterTranslationMaxBlocks: 50,
        customPipeline: { translator: false },
      }),
    ).toEqual({})

    expect(
      chapterTranslationPipelineOptions({
        chapterContextTranslation: true,
        chapterTranslationTokenBudget: 2048,
        chapterTranslationMaxBlocks: 50,
        customPipeline: { translator: true },
      }),
    ).toEqual({
      chapterContextTranslation: true,
      chapterTranslationTokenBudget: 2048,
      chapterTranslationMaxBlocks: 50,
    })
  })
})
