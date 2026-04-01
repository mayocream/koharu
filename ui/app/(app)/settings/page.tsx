'use client'

import Link from 'next/link'
import { useTranslation } from 'react-i18next'
import { ChevronLeftIcon, ChevronRightIcon } from 'lucide-react'
import { ScrollArea } from '@/components/ui/scroll-area'
import {
  ApiKeysSection,
  AppearanceSection,
  DeviceSection,
  LanguageSection,
  LocalLlmSection,
  ProxySection,
} from './settings-sections'
import { useSettingsController } from './use-settings-controller'

export default function SettingsPage() {
  const { t, i18n } = useTranslation()
  const {
    theme,
    setTheme,
    locales,
    deviceInfo,
    apiKeys,
    localLlm,
    activeConfig,
    visibleKeys,
    showAdvanced,
    setShowAdvanced,
    pingState,
    bootstrapConfig,
    toggleVisibleKey,
    handleApiKeyChange,
    flushApiKeySave,
    handleProxyChange,
    flushProxySave,
    handleTestConnection,
    handlePresetChange,
    updateLocalLlm,
  } = useSettingsController()

  return (
    <div className='bg-muted flex min-h-0 flex-1 flex-col overflow-hidden'>
      <ScrollArea className='min-h-0 flex-1' viewportClassName='h-full'>
        <div className='min-h-full px-4 py-6'>
          <div className='relative mx-auto max-w-xl'>
            <div className='mb-8 flex items-center'>
              <Link
                href='/'
                prefetch={false}
                className='text-muted-foreground hover:bg-accent hover:text-foreground absolute -left-14 flex size-10 items-center justify-center rounded-full transition'
              >
                <ChevronLeftIcon className='size-6' />
              </Link>
              <h1 className='text-foreground text-2xl font-bold'>
                {t('settings.title')}
              </h1>
            </div>

            <AppearanceSection t={t} theme={theme} setTheme={setTheme} />

            <LanguageSection
              t={t}
              locales={locales}
              language={i18n.language}
              onLanguageChange={i18n.changeLanguage}
            />

            <ProxySection
              t={t}
              bootstrapConfig={bootstrapConfig}
              onProxyChange={handleProxyChange}
              onProxyBlur={() => {
                void flushProxySave()
              }}
            />

            {deviceInfo ? (
              <DeviceSection t={t} mlDevice={deviceInfo.mlDevice} />
            ) : null}

            <ApiKeysSection
              t={t}
              apiKeys={apiKeys}
              visibleKeys={visibleKeys}
              onApiKeyChange={handleApiKeyChange}
              onApiKeyBlur={(provider) => {
                void flushApiKeySave(provider)
              }}
              onToggleVisibility={toggleVisibleKey}
            />

            <LocalLlmSection
              t={t}
              localLlm={localLlm}
              activeConfig={activeConfig}
              showAdvanced={showAdvanced}
              pingState={pingState}
              visibleApiKey={!!visibleKeys[`llm-${localLlm.activePreset}`]}
              onToggleAdvanced={() => setShowAdvanced((current) => !current)}
              onToggleApiKeyVisibility={() =>
                toggleVisibleKey(`llm-${localLlm.activePreset}`)
              }
              onPresetChange={handlePresetChange}
              onConfigChange={updateLocalLlm}
              onTestConnection={() => {
                void handleTestConnection()
              }}
            />

            <div className='border-border mb-8 border-t' />

            <Link
              href='/about'
              prefetch={false}
              className='hover:bg-accent flex w-full items-center justify-between rounded-lg px-3 py-3 text-left transition'
            >
              <span className='text-foreground text-sm font-medium'>
                {t('settings.about')}
              </span>
              <ChevronRightIcon className='text-muted-foreground size-5' />
            </Link>
          </div>
        </div>
      </ScrollArea>
    </div>
  )
}
