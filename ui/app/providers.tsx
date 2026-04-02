'use client'

import { useEffect, type ReactNode } from 'react'
import { I18nextProvider } from 'react-i18next'
import { ThemeProvider } from 'next-themes'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import ClientOnly from '@/components/ClientOnly'
import { TooltipProvider } from '@/components/ui/tooltip'
import { isTauri } from '@/lib/backend'
import { useGetApiKey } from '@/lib/api/providers/providers'
import i18n from '@/lib/i18n'
import { usePreferencesStore } from '@/lib/stores/preferencesStore'
import { ProcessingProvider } from '@/lib/machines'

const queryClient = new QueryClient()

const API_KEY_PROVIDERS = ['openai', 'openai-compatible', 'gemini', 'claude', 'deepseek'] as const

function ApiKeySyncer() {
  const setApiKey = usePreferencesStore((state) => state.setApiKey)

  return (
    <>
      {API_KEY_PROVIDERS.map((provider) => (
        <ApiKeySync key={provider} provider={provider} setApiKey={setApiKey} />
      ))}
    </>
  )
}

function ApiKeySync({
  provider,
  setApiKey,
}: {
  provider: string
  setApiKey: (provider: string, key: string) => void
}) {
  const { data, status } = useGetApiKey(provider, {
    query: {
      enabled: isTauri(),
      select: (res: { apiKey?: string | null }) => res?.apiKey ?? '',
    },
  })

  useEffect(() => {
    if (status === 'success') setApiKey(provider, data ?? '')
  }, [data, status, provider, setApiKey])

  return null
}

export function Providers({ children }: { children: ReactNode }) {
  useEffect(() => {
    const onLang = (lng: string) => { document.documentElement.lang = lng }
    onLang(i18n.language)
    i18n.on('languageChanged', onLang)
    return () => { i18n.off('languageChanged', onLang) }
  }, [])

  return (
    <QueryClientProvider client={queryClient}>
      <ProcessingProvider>
        <ThemeProvider attribute='class' defaultTheme='system' enableSystem>
          <ClientOnly>
            <ApiKeySyncer />
            <I18nextProvider i18n={i18n}>
              <TooltipProvider delayDuration={0}>{children}</TooltipProvider>
            </I18nextProvider>
          </ClientOnly>
        </ThemeProvider>
      </ProcessingProvider>
    </QueryClientProvider>
  )
}

export default Providers
