import type { RenderEffect, RgbaColor, TextStyle } from '@/types'

export const DEFAULT_COLOR: RgbaColor = [0, 0, 0, 255]
export const DEFAULT_FONT_FAMILIES = ['Arial']

export const RENDER_EFFECTS: readonly RenderEffect[] = [
  'normal',
  'antique',
  'metal',
  'manga',
  'motionBlur',
]

export const clampByte = (value: number) =>
  Math.max(0, Math.min(255, Math.round(value)))

export const colorToHex = (color: RgbaColor) =>
  `#${color
    .slice(0, 3)
    .map((value) => value.toString(16).padStart(2, '0'))
    .join('')}`

export const hexToColor = (value: string, alpha: number): RgbaColor => {
  const normalized = value.replace('#', '')
  if (normalized.length !== 6) {
    return [0, 0, 0, clampByte(alpha)]
  }

  const r = Number.parseInt(normalized.slice(0, 2), 16)
  const g = Number.parseInt(normalized.slice(2, 4), 16)
  const b = Number.parseInt(normalized.slice(4, 6), 16)

  if ([r, g, b].some((channel) => Number.isNaN(channel))) {
    return [0, 0, 0, clampByte(alpha)]
  }

  return [r, g, b, clampByte(alpha)]
}

export const uniqueStrings = (values: string[]) => {
  const seen = new Set<string>()
  return values.filter((value) => {
    if (!value || seen.has(value)) return false
    seen.add(value)
    return true
  })
}

export const buildTextStyle = (
  style: TextStyle | undefined,
  updates: Partial<TextStyle>,
  fallbackFontFamilies: string[],
  fallbackColor: RgbaColor,
): TextStyle => ({
  fontFamilies:
    updates.fontFamilies ?? style?.fontFamilies ?? fallbackFontFamilies,
  fontSize: updates.fontSize ?? style?.fontSize,
  color: updates.color ?? style?.color ?? fallbackColor,
  effect: updates.effect ?? style?.effect,
})

export const mergeFontFamilies = (
  nextFont: string,
  current: string[] | undefined,
  fallbackFontFamilies: string[],
) => {
  const base = current?.length ? current : fallbackFontFamilies
  return [nextFont, ...base.filter((family) => family !== nextFont)]
}
