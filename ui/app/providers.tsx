'use client'

import { ThemeProvider } from 'next-themes'
import { useEffect, type ReactNode } from 'react'
import { I18nextProvider } from 'react-i18next'

import ClientOnly from '@/components/ClientOnly'
import { TooltipProvider } from '@/components/ui/tooltip'
import i18n from '@/lib/i18n'
import { koharuClient } from '@/lib/koharu'

export function Providers({ children }: { children: ReactNode }) {
  useEffect(() => koharuClient.connect(), [])

  useEffect(() => {
    const setLanguage = (language: string) => {
      document.documentElement.lang = language
    }
    setLanguage(i18n.language)
    i18n.on('languageChanged', setLanguage)
    return () => i18n.off('languageChanged', setLanguage)
  }, [])

  return (
    <ThemeProvider attribute='class' defaultTheme='system' enableSystem>
      <ClientOnly>
        <I18nextProvider i18n={i18n}>
          <TooltipProvider delayDuration={0}>{children}</TooltipProvider>
        </I18nextProvider>
      </ClientOnly>
    </ThemeProvider>
  )
}

export default Providers
