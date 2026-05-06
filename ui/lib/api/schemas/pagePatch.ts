export interface PagePatch {
  /**
   * @minimum 0
   * @nullable
   */
  height?: number | null
  /** @nullable */
  name?: string | null
  /**
   * @minimum 0
   * @nullable
   */
  width?: number | null
}
