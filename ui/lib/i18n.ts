'use client'

import i18n, { type Resource } from 'i18next'
import { initReactI18next } from 'react-i18next'
import LanguageDetector from 'i18next-browser-languagedetector'
import LocalStorageBackend from 'i18next-localstorage-backend'

import enUS from '@/public/locales/en-US/translation.json'
import zhCN from '@/public/locales/zh-CN/translation.json'
import zhTW from '@/public/locales/zh-TW/translation.json'
import jaJP from '@/public/locales/ja-JP/translation.json'

const resources = {
  'en-US': { translation: enUS },
  'zh-CN': { translation: zhCN },
  'zh-TW': { translation: zhTW },
  'ja-JP': { translation: jaJP },
} satisfies Resource

i18n
  .use(LocalStorageBackend)
  .use(LanguageDetector)
  .use(initReactI18next)
  .init({
    resources,

    fallbackLng: 'en-US',
    interpolation: {
      escapeValue: false, // not needed for react as it escapes by default
    },

    react: {
      useSuspense: false,
    },
  })

export default i18n
