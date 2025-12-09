'use client'

import { useEffect, type ReactNode } from 'react'
import { I18nextProvider } from 'react-i18next'
import { Tooltip } from 'radix-ui'
import i18n, {
  getPreferredLocale,
  locales,
  persistLocale,
  type LocaleCode,
} from '@/lib/i18n'

export function Providers({ children }: { children: ReactNode }) {
  useEffect(() => {
    const preferred = getPreferredLocale()
    if (preferred && preferred !== i18n.language) {
      void i18n.changeLanguage(preferred)
    }

    const handleLanguageChange = (lng: string) => {
      const nextLocale: LocaleCode = locales.includes(lng as LocaleCode)
        ? (lng as LocaleCode)
        : locales[0]
      document.documentElement.lang = nextLocale
      persistLocale(nextLocale)
    }

    handleLanguageChange(i18n.language)
    i18n.on('languageChanged', handleLanguageChange)
    return () => {
      i18n.off('languageChanged', handleLanguageChange)
    }
  }, [])

  return (
    <I18nextProvider i18n={i18n}>
      <Tooltip.Provider delayDuration={300}>{children}</Tooltip.Provider>
    </I18nextProvider>
  )
}

export default Providers
