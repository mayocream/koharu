import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

const values = new Map<string, string>()
const storage: Storage = {
  get length() {
    return values.size
  },
  clear: () => values.clear(),
  getItem: (key) => values.get(key) ?? null,
  key: (index) => [...values.keys()][index] ?? null,
  removeItem: (key) => values.delete(key),
  setItem: (key, value) => values.set(key, value),
}

describe('interface language persistence', () => {
  beforeEach(() => {
    Object.defineProperty(window, 'localStorage', { configurable: true, value: storage })
  })

  afterEach(() => {
    window.localStorage.clear()
    vi.resetModules()
  })

  it('stores an explicitly selected language', async () => {
    vi.resetModules()
    const { default: i18n } = await import('@/lib/i18n')

    await i18n.changeLanguage('ja-JP')

    expect(window.localStorage.getItem('i18nextLng')).toBe('ja-JP')
  })

  it('restores the saved language when i18n initializes', async () => {
    window.localStorage.setItem('i18nextLng', 'ja-JP')
    vi.resetModules()

    const { default: i18n } = await import('@/lib/i18n')

    expect(i18n.resolvedLanguage).toBe('ja-JP')
    expect(window.localStorage.getItem('i18nextLng')).toBe('ja-JP')
  })
})
