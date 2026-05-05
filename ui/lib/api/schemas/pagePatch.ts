import type { ReadingOrder } from './readingOrder'

export interface PagePatch {
  /**
   * @minimum 0
   * @nullable
   */
  height?: number | null
  /** @nullable */
  name?: string | null
  /** @nullable */
  readingOrder?: ReadingOrder | null
  /**
   * @minimum 0
   * @nullable
   */
  width?: number | null
}
