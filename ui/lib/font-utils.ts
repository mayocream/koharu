import { type FontFaceInfo } from '@/lib/api/schemas'

export const STYLE_KEYWORDS = [
  'Bold',
  'Italic',
  'Light',
  'Medium',
  'Regular',
  'Condensed',
  'Black',
  'Thin',
  'ExtraBold',
  'ExtraLight',
  'Semibold',
  'Semilight',
  'DemiBold',
  'Demibold',
  'Extra',
  'Ultra',
  'UltraBold',
  'UltraLight',
  'Heavy',
  'Book',
  'Normal',
  'Oblique',
]

export const WEIGHT_NAMES: Record<string, string> = {
  '100': 'render.fontWeights.thin',
  '200': 'render.fontWeights.extraLight',
  '300': 'render.fontWeights.light',
  '400': 'render.fontWeights.regular',
  '500': 'render.fontWeights.medium',
  '600': 'render.fontWeights.semiBold',
  '700': 'render.fontWeights.bold',
  '800': 'render.fontWeights.extraBold',
  '900': 'render.fontWeights.black',
}

export const normalizeFamilyName = (name: string) => {
  let normalized = name.trim()
  // Sort keywords by length descending to match longer ones (ExtraBold) before shorter ones (Bold)
  const keywords = [...STYLE_KEYWORDS].sort((a, b) => b.length - a.length)
  // Remove common style suffixes. Handles "Arial Bold", "Arial-Bold", and "ArialBold".
  const regex = new RegExp(`[\\s\\-_]?(${keywords.join('|')})$`, 'i')
  normalized = normalized.replace(regex, '').trim()
  return normalized
}

export const uniqueFontFaces = (values: FontFaceInfo[]) => {
  const seen = new Set<string>()
  return values.filter((v) => {
    if (!v.postScriptName || seen.has(v.postScriptName)) return false
    seen.add(v.postScriptName)
    return true
  })
}

export const findFontFace = (fonts: FontFaceInfo[], value?: string) => {
  if (!value) return undefined
  return fonts.find(
    (f) =>
      f.postScriptName === value || f.familyName === value || f.familyName.trim() === value.trim(),
  )
}

export const escapeRegExp = (string: string) => {
  return string.replace(/[.*+?^${}$()|[\]\\]/g, '\\$&')
}

/**
 * Generates a localized label for a font variant.
 * @param face The font face info
 * @param t The translation function (i18next)
 */
export const getLocalizedFontLabel = (
  face: FontFaceInfo,
  t: (key: string, options?: any) => string,
) => {
  const familyNorm = face.familyName.replace(/[\s\-_]+/g, '').toLowerCase()
  const familyRegex = new RegExp(`^${escapeRegExp(familyNorm)}`, 'i')
  let psNorm = face.postScriptName.replace(/[\s\-_]+/g, '')

  // Try stripping by family name (space-agnostic)
  let l = psNorm.replace(familyRegex, '')

  // If it didn't strip properly, fallback to colon split
  if (l === psNorm && face.postScriptName.includes(':')) {
    l = face.postScriptName.split(':').pop() || l
  }

  l = l.replace(/^[:\-_]+/, '')

  // Map numeric weights to human names
  const weightMatch = l.match(/(\d+)/)
  if (weightMatch) {
    const weight = weightMatch[1]
    const isItalic = /i(talic)?$/i.test(l)
    const key = WEIGHT_NAMES[weight]
    if (key) {
      const name = t(key)
      return isItalic ? t('render.fontStyles.italicWithName', { name }) : name
    }
  }

  let res = l.replace(/MT$/, '').replace(/PS$/, '') || t('render.fontWeights.regular')
  if (res.toLowerCase() === 'italic') res = t('render.fontStyles.regularItalic')
  return res
}
