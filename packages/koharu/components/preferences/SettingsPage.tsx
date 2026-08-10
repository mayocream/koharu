'use client'

import {
  ArrowLeft,
  Cpu,
  KeyRound,
  Keyboard,
  Languages,
  Monitor,
  Moon,
  Palette,
  Sun,
  Type,
} from 'lucide-react'
import { useTheme } from 'next-themes'
import { useEffect, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'

import { PipelinePreferences } from '@/components/preferences/PipelinePreferences'
import {
  PreferencePage,
  PreferenceRow,
  PreferenceSection,
} from '@/components/preferences/PreferenceFields'
import { ProviderPreferences } from '@/components/preferences/ProviderPreferences'
import { TranslationPreferences } from '@/components/preferences/TranslationPreferences'
import { TypesettingPreferences } from '@/components/preferences/TypesettingPreferences'
import { call, refreshPreferences, refreshTranslationModels } from '@/lib/backend'
import { supportedLanguages } from '@/lib/i18n'
import {
  commands,
  type PipelineConfig,
  type ProviderPreferences as ProviderSettings,
  type TypesettingConfig,
} from '@/lib/protocol'
import { receivePreferences, useKoharuStore, type ShortcutAction } from '@/lib/store'
import { Button } from '@koharu/ui/components/button'
import { Input } from '@koharu/ui/components/input'
import { ScrollArea } from '@koharu/ui/components/scroll-area'
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@koharu/ui/components/select'

const tabs = [
  ['appearance', Palette, 'Appearance'],
  ['pipeline', Cpu, 'Pipeline'],
  ['providers', KeyRound, 'Providers'],
  ['translation', Languages, 'Translation'],
  ['typesetting', Type, 'Typesetting'],
  ['shortcuts', Keyboard, 'Shortcuts'],
] as const
type Tab = (typeof tabs)[number][0]

export function SettingsPage() {
  const open = useKoharuStore((state) => state.settingsOpen)
  const setOpen = useKoharuStore((state) => state.setSettingsOpen)
  const preferences = useKoharuStore((state) => state.preferences)
  const translationModels = useKoharuStore((state) => state.translationModels)
  const [tab, setTab] = useState<Tab>('appearance')
  const [pipeline, setPipeline] = useState<PipelineConfig | null>(preferences?.pipeline ?? null)
  const [providers, setProviders] = useState<ProviderSettings | null>(
    preferences?.providers ?? null,
  )
  const [typesetting, setTypesetting] = useState<TypesettingConfig | null>(
    preferences?.typesetting ?? null,
  )
  const translation = pipeline?.translation ?? null
  const lastSaved = useRef<string | null>(null)
  const lastSavedProviders = useRef<string | null>(null)

  useEffect(() => {
    if (!open) return
    void refreshPreferences().catch(() => undefined)
    void refreshTranslationModels().catch(() => undefined)
  }, [open])

  useEffect(() => {
    if (!open) return
    setPipeline(preferences?.pipeline ?? null)
    setProviders(preferences?.providers ?? null)
    setTypesetting(preferences?.typesetting ?? null)
    if (preferences) {
      lastSaved.current = JSON.stringify([
        preferences.pipeline,
        preferences.providers,
        preferences.typesetting,
      ])
      lastSavedProviders.current = JSON.stringify(preferences.providers)
    }
  }, [open, preferences])

  useEffect(() => {
    if (!open || !pipeline || !providers || !typesetting) return
    const serialized = JSON.stringify([pipeline, providers, typesetting])
    if (serialized === lastSaved.current) return
    const serializedProviders = JSON.stringify(providers)
    const timeout = window.setTimeout(() => {
      lastSaved.current = serialized
      const providersChanged = serializedProviders !== lastSavedProviders.current
      void call(commands.savePreferences, pipeline, providers, typesetting)
        .then((saved) => {
          receivePreferences(saved)
          if (providersChanged) {
            lastSavedProviders.current = serializedProviders
            void refreshTranslationModels(true).catch(() => undefined)
          }
        })
        .catch(() => undefined)
    }, 260)
    return () => window.clearTimeout(timeout)
  }, [open, pipeline, providers, typesetting])

  if (!open) return null

  return (
    <section className='settings-page flex min-h-0 flex-1 bg-[var(--surface-sidebar)] text-foreground'>
      <nav className='flex w-64 shrink-0 flex-col bg-[var(--surface-sidebar)] px-3 py-4'>
        <Button
          type='button'
          variant='ghost'
          className='mb-5 h-9 justify-start gap-2 rounded-lg px-2 text-[12px] text-muted-foreground hover:bg-foreground/[0.06] hover:text-foreground'
          onClick={() => setOpen(false)}
        >
          <ArrowLeft className='size-4' /> Back to editor
        </Button>
        <p className='mb-2 px-2 text-[10px] font-semibold tracking-[0.14em] text-muted-foreground uppercase'>
          Settings
        </p>
        <div className='grid gap-1'>
          {tabs.map(([id, Icon, label]) => (
            <Button
              key={id}
              type='button'
              variant='ghost'
              size='sm'
              data-active={tab === id}
              className='h-10 w-full justify-start gap-3 rounded-lg px-3 text-left text-[12px] text-muted-foreground hover:bg-foreground/[0.06] hover:text-foreground data-[active=true]:bg-accent data-[active=true]:text-accent-foreground'
              onClick={() => setTab(id)}
            >
              <Icon className='size-4' /> {label}
            </Button>
          ))}
        </div>
      </nav>
      <main className='relative z-10 flex min-w-0 flex-1 flex-col overflow-hidden rounded-tl-2xl bg-[var(--surface-canvas)] shadow-[var(--shadow-content)]'>
        <header className='flex h-14 shrink-0 items-center border-b border-border/80 px-8'>
          <h1 className='text-[13px] font-semibold tracking-[-0.02em]'>Settings</h1>
        </header>
        <ScrollArea className='min-h-0 flex-1'>
          <div className='mx-auto w-full max-w-4xl px-10 py-10'>
            {tab === 'appearance' && <AppearancePreferences />}
            {tab === 'pipeline' &&
              (pipeline ? (
                <PipelinePreferences value={pipeline} onChange={setPipeline} />
              ) : (
                <LoadingPreferences />
              ))}
            {tab === 'providers' &&
              (providers ? (
                <ProviderPreferences value={providers} onChange={setProviders} />
              ) : (
                <LoadingPreferences />
              ))}
            {tab === 'translation' &&
              (translation ? (
                <TranslationPreferences
                  value={translation}
                  modelChoices={translationModels}
                  providers={providers?.entries ?? []}
                  languages={preferences?.languages ?? []}
                  onChange={(translation) =>
                    setPipeline((current) => (current ? { ...current, translation } : current))
                  }
                />
              ) : (
                <LoadingPreferences />
              ))}
            {tab === 'typesetting' &&
              (typesetting ? (
                <TypesettingPreferences value={typesetting} onChange={setTypesetting} />
              ) : (
                <LoadingPreferences />
              ))}
            {tab === 'shortcuts' && <ShortcutPreferences />}
          </div>
        </ScrollArea>
      </main>
    </section>
  )
}

function AppearancePreferences() {
  const { theme, setTheme } = useTheme()
  const { t, i18n } = useTranslation()
  const themes = [
    ['light', Sun, 'Light'],
    ['dark', Moon, 'Dark'],
    ['system', Monitor, 'System'],
  ] as const

  return (
    <PreferencePage
      title='Appearance'
      description='Match your workspace to the room without changing how artwork is rendered.'
    >
      <PreferenceSection title='Interface'>
        <PreferenceRow title='Theme' description='System follows the operating-system setting.'>
          <div className='grid grid-cols-3 rounded-xl border border-input bg-background p-0.5'>
            {themes.map(([value, Icon, label]) => (
              <Button
                key={value}
                type='button'
                variant='ghost'
                size='sm'
                data-active={theme === value}
                className='h-9 gap-1.5 text-[10px] text-muted-foreground hover:text-foreground data-[active=true]:bg-primary data-[active=true]:text-primary-foreground data-[active=true]:hover:bg-primary/90 data-[active=true]:hover:text-primary-foreground'
                onClick={() => setTheme(value)}
              >
                <Icon className='size-3.5' /> {label}
              </Button>
            ))}
          </div>
        </PreferenceRow>
        <PreferenceRow title='Language'>
          <Select
            value={i18n.language}
            items={Object.fromEntries(
              supportedLanguages.map((language) => [
                language,
                t(`menu.languages.${language}`, { defaultValue: language }),
              ]),
            )}
            onValueChange={(language) => language && i18n.changeLanguage(language)}
          >
            <SelectTrigger className='h-8 w-full text-[11px]'>
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              {supportedLanguages.map((language) => (
                <SelectItem key={language} value={language}>
                  {t(`menu.languages.${language}`, { defaultValue: language })}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
        </PreferenceRow>
      </PreferenceSection>
    </PreferencePage>
  )
}

function ShortcutPreferences() {
  const shortcuts = useKoharuStore((state) => state.shortcuts)
  const setShortcut = useKoharuStore((state) => state.setShortcut)
  const actions: ShortcutAction[] = [
    'select',
    'text',
    'draw',
    'eraser',
    'color_picker',
    'remove',
    'pan',
    'fit',
  ]
  return (
    <PreferencePage
      title='Shortcuts'
      description='Single-key shortcuts are available whenever a text field is not focused.'
    >
      <PreferenceSection title='Tools'>
        {actions.map((action) => (
          <PreferenceRow key={action} title={shortcutName(action)}>
            <Input
              aria-label={`${shortcutName(action)} shortcut`}
              maxLength={1}
              value={shortcuts[action]}
              className='ml-auto h-8 w-14 text-center text-[12px] uppercase'
              onChange={(event) => setShortcut(action, event.currentTarget.value)}
            />
          </PreferenceRow>
        ))}
      </PreferenceSection>
    </PreferencePage>
  )
}

function LoadingPreferences() {
  return <p className='py-10 text-[12px] text-muted-foreground'>Loading preferences...</p>
}

function shortcutName(action: ShortcutAction): string {
  const names: Record<ShortcutAction, string> = {
    select: 'Select',
    text: 'Text',
    draw: 'Brush',
    eraser: 'Eraser',
    color_picker: 'Sample color',
    remove: 'Remove',
    pan: 'Pan',
    fit: 'Fit canvas',
  }
  return names[action]
}
