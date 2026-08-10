import { describe, expect, it } from 'vitest'

import { resources } from '@/lib/i18n'
import { orderedLanguageChoices } from '@/lib/translation'

function keys(value: object, prefix = ''): string[] {
  return Object.entries(value).flatMap(([key, item]) => {
    const path = prefix ? `${prefix}.${key}` : key
    return item && typeof item === 'object' ? keys(item, path) : [path]
  })
}

describe('native editor localization', () => {
  it('defines every new visible label in every preserved locale', () => {
    const expected = keys(resources['en-US'].translation.native).sort()
    for (const [locale, resource] of Object.entries(resources)) {
      expect(keys(resource.translation.native).sort(), locale).toEqual(expected)
    }
  })

  it('orders language choices by their displayed name without mutating the source', () => {
    const languages = [
      { tag: 'ja-JP', name: 'Japanese' },
      { tag: 'zh-CN', name: 'Simplified Chinese' },
      { tag: 'en-US', name: 'English' },
      { tag: 'de-DE', name: 'German' },
    ]

    expect(orderedLanguageChoices(languages).map((language) => language.name)).toEqual([
      'English',
      'German',
      'Japanese',
      'Simplified Chinese',
    ])
    expect(languages.map((language) => language.name)).toEqual([
      'Japanese',
      'Simplified Chinese',
      'English',
      'German',
    ])
  })
})
