'use client'

import { useEffect, type ReactNode } from 'react'
import { I18nextProvider } from 'react-i18next'
import { Tooltip } from 'radix-ui'
import { invoke, listen } from '@/lib/backend'
import i18n, {
  getPreferredLocale,
  locales,
  persistLocale,
  type LocaleCode,
} from '@/lib/i18n'
import { useAppStore } from '@/lib/store'

export function Providers({ children }: { children: ReactNode }) {
  const setTotalPages = useAppStore((state) => state.setTotalPages)

  useEffect(() => {
    let unlisten: (() => void) | undefined
    ;(async () => {
      try {
        const count = await invoke<number>('get_documents')
        if (count > 0) {
          setTotalPages(count)
        }
      } catch (_) {}

      try {
        unlisten = await listen<number>('documents:opened', (event) => {
          setTotalPages(event.payload ?? 0)
        })
      } catch (_) {}
    })()

    return () => {
      unlisten?.()
    }
  }, [setTotalPages])

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
