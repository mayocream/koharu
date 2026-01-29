'use client'

import { useEffect, useState, type ReactNode } from 'react'
import { I18nextProvider } from 'react-i18next'
import { ThemeProvider } from 'next-themes'
import { TooltipProvider } from '@/components/ui/tooltip'
import { invoke } from '@/lib/backend'
import i18n from '@/lib/i18n'
import { useAppStore } from '@/lib/store'

export function Providers({ children }: { children: ReactNode }) {
  const [mounted, setMounted] = useState(false)
  const setTotalPages = useAppStore((state) => state.setTotalPages)

  useEffect(() => {
    ;(async () => {
      try {
        const count = await invoke<number>('get_documents')
        if (count > 0) {
          setTotalPages(count)
        }
      } catch (_) {}
    })()
  }, [setTotalPages])

  useEffect(() => {
    setMounted(true)

    const handleLanguageChange = (lng: string) => {
      document.documentElement.lang = lng
    }

    handleLanguageChange(i18n.language)
    i18n.on('languageChanged', handleLanguageChange)
    return () => {
      i18n.off('languageChanged', handleLanguageChange)
    }
  }, [])

  if (!mounted) return null

  return (
    <I18nextProvider i18n={i18n}>
      <ThemeProvider attribute='class' defaultTheme='system' enableSystem>
        <TooltipProvider delayDuration={0}>{children}</TooltipProvider>
      </ThemeProvider>
    </I18nextProvider>
  )
}

export default Providers
