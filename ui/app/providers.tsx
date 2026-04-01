'use client'

import { useEffect, type ReactNode } from 'react'
import { I18nextProvider } from 'react-i18next'
import { QueryClientProvider } from '@tanstack/react-query'
import { ThemeProvider } from 'next-themes'
import ClientOnly from '@/components/ClientOnly'
import { TooltipProvider } from '@/components/ui/tooltip'
import i18n from '@/lib/i18n'
import { getQueryClient } from '@/lib/react-query/client'
import { ProvidersBootstrap } from './providers-bootstrap'

export function Providers({ children }: { children: ReactNode }) {
  const queryClient = getQueryClient()

  useEffect(() => {
    const handleLanguageChange = (language: string) => {
      document.documentElement.lang = language
    }

    handleLanguageChange(i18n.language)
    i18n.on('languageChanged', handleLanguageChange)

    return () => {
      i18n.off('languageChanged', handleLanguageChange)
    }
  }, [])

  return (
    <QueryClientProvider client={queryClient}>
      <ThemeProvider attribute='class' defaultTheme='system' enableSystem>
        <ClientOnly>
          <ProvidersBootstrap>
            <I18nextProvider i18n={i18n}>
              <TooltipProvider delayDuration={0}>{children}</TooltipProvider>
            </I18nextProvider>
          </ProvidersBootstrap>
        </ClientOnly>
      </ThemeProvider>
    </QueryClientProvider>
  )
}

export default Providers
