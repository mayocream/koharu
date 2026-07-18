import { describe, expect, it } from 'vitest'

import { resources } from '@/lib/i18n'

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
})
