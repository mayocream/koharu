'use client'

import i18n, { type Resource } from 'i18next'
import { initReactI18next } from 'react-i18next'
import enUS from '../public/locales/en-US/translation.json'
import zhCN from '../public/locales/zh-CN/translation.json'
import zhTW from '../public/locales/zh-TW/translation.json'
import jaJP from '../public/locales/ja-JP/translation.json'

export const locales = ['en-US', 'zh-CN', 'zh-TW', 'ja-JP'] as const
export type LocaleCode = (typeof locales)[number]
export const defaultLocale: LocaleCode = 'en-US'

const STORAGE_KEY = 'koharu-language'

const resources = {
  'en-US': { translation: enUS },
  'zh-CN': { translation: zhCN },
  'zh-TW': { translation: zhTW },
  'ja-JP': { translation: jaJP },
} satisfies Resource

const detectLocale = (): LocaleCode => defaultLocale

if (!i18n.isInitialized) {
  void i18n.use(initReactI18next).init({
    resources,
    lng: detectLocale(),
    fallbackLng: defaultLocale,
    supportedLngs: locales,
    interpolation: { escapeValue: false },
    returnEmptyString: false,
    react: { useSuspense: false },
    defaultNS: 'translation',
  })
}

export const getPreferredLocale = (): LocaleCode | undefined => {
  if (typeof window === 'undefined') return undefined

  try {
    const stored = localStorage.getItem(STORAGE_KEY) as LocaleCode | null
    if (stored && locales.includes(stored)) return stored
  } catch (_) {}

  const browser = (navigator.language || '').toLowerCase()
  if (browser.startsWith('zh')) {
    return browser.includes('tw') ||
      browser.includes('hk') ||
      browser.includes('mo')
      ? 'zh-TW'
      : 'zh-CN'
  }
  if (browser.startsWith('ja')) return 'ja-JP'
  const exact = locales.find((locale) => locale.toLowerCase() === browser)
  return exact
}

export const persistLocale = (locale: LocaleCode) => {
  if (!locales.includes(locale)) return
  try {
    localStorage.setItem(STORAGE_KEY, locale)
  } catch (_) {}
}

export default i18n
