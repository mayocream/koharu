export const CHAPTER_TOKEN_BUDGET_DEFAULT = 4096
export const CHAPTER_TOKEN_BUDGET_MIN = 256
export const CHAPTER_TOKEN_BUDGET_MAX = 8192

export const CHAPTER_MAX_BLOCKS_DEFAULT = 100
export const CHAPTER_MAX_BLOCKS_MIN = 1
export const CHAPTER_MAX_BLOCKS_MAX = 200

export function clampChapterTranslationTokenBudget(value: number): number {
  if (!Number.isFinite(value)) return CHAPTER_TOKEN_BUDGET_DEFAULT
  return Math.min(
    CHAPTER_TOKEN_BUDGET_MAX,
    Math.max(CHAPTER_TOKEN_BUDGET_MIN, Math.round(value)),
  )
}

export function clampChapterTranslationMaxBlocks(value: number): number {
  if (!Number.isFinite(value)) return CHAPTER_MAX_BLOCKS_DEFAULT
  return Math.min(CHAPTER_MAX_BLOCKS_MAX, Math.max(CHAPTER_MAX_BLOCKS_MIN, Math.round(value)))
}

export function chapterTranslationPipelineOptions(prefs: {
  chapterContextTranslation: boolean
  chapterTranslationTokenBudget: number
  chapterTranslationMaxBlocks: number
  customPipeline?: { translator: boolean }
}): {
  chapterContextTranslation?: boolean
  chapterTranslationTokenBudget?: number
  chapterTranslationMaxBlocks?: number
} {
  const enabled =
    prefs.chapterContextTranslation &&
    (prefs.customPipeline?.translator === undefined || prefs.customPipeline.translator)
  if (!enabled) return {}
  return {
    chapterContextTranslation: true,
    chapterTranslationTokenBudget: prefs.chapterTranslationTokenBudget,
    chapterTranslationMaxBlocks: prefs.chapterTranslationMaxBlocks,
  }
}
