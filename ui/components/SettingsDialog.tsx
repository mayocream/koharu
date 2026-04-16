'use client'

import { useQueryClient } from '@tanstack/react-query'
import {
  SunIcon,
  MoonIcon,
  MonitorIcon,
  CheckCircleIcon,
  AlertCircleIcon,
  LoaderIcon,
  PaletteIcon,
  KeyIcon,
  HardDriveIcon,
  InfoIcon,
  CpuIcon,
  KeyboardIcon,
  SaveIcon,
  RotateCcwIcon,
  AlertTriangleIcon,
} from 'lucide-react'
import { useTheme } from 'next-themes'
import { useEffect, useMemo, useRef, useState, type ReactNode } from 'react'
import { useTranslation } from 'react-i18next'

import {
  Accordion,
  AccordionItem,
  AccordionTrigger,
  AccordionContent,
} from '@/components/ui/accordion'
import {
  AlertDialog,
  AlertDialogAction,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogTitle,
} from '@/components/ui/alert-dialog'
import { Button } from '@/components/ui/button'
import { Dialog, DialogContent, DialogTitle, DialogDescription } from '@/components/ui/dialog'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { ScrollArea } from '@/components/ui/scroll-area'
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select'
import { useUpdater, type UpdaterStatus } from '@/components/Updater'
import { getLlmCatalog, getGetLlmCatalogQueryKey } from '@/lib/api/llm/llm'
import type {
  UpdateConfigBody,
  ProviderConfig,
  LlmProviderCatalog,
  GetEngineCatalog200,
} from '@/lib/api/schemas'
import { getConfig, getEngineCatalog, getMeta, updateConfig } from '@/lib/api/system/system'
import { isTauri, openExternalUrl } from '@/lib/backend'
import { supportedLanguages } from '@/lib/i18n'
import {
  getPlatform,
  formatShortcut,
  isKeyBlocked,
  areShortcutsEqual,
  isModifierKey,
} from '@/lib/shortcutUtils'
import { usePreferencesStore } from '@/lib/stores/preferencesStore'

const GITHUB_REPO = 'mayocream/koharu'

const TABS = [
  { id: 'appearance', icon: PaletteIcon, labelKey: 'settings.appearance' },
  { id: 'engines', icon: CpuIcon, labelKey: 'settings.engines' },
  { id: 'providers', icon: KeyIcon, labelKey: 'settings.apiKeys' },
  { id: 'keybinds', icon: KeyboardIcon, labelKey: 'settings.keybinds' },
  { id: 'runtime', icon: HardDriveIcon, labelKey: 'settings.runtime' },
  { id: 'about', icon: InfoIcon, labelKey: 'settings.about' },
] as const

export type TabId = (typeof TABS)[number]['id']

type SettingsDialogProps = {
  open: boolean
  onOpenChange: (open: boolean) => void
  defaultTab?: TabId
}

const DEFAULT_HTTP_CONNECT_TIMEOUT = 20
const DEFAULT_HTTP_READ_TIMEOUT = 300
const DEFAULT_HTTP_MAX_RETRIES = 3

export function SettingsDialog({
  open,
  onOpenChange,
  defaultTab = 'appearance',
}: SettingsDialogProps) {
  const { t } = useTranslation()
  const queryClient = useQueryClient()
  const [tab, setTab] = useState<TabId>(defaultTab)
  useEffect(() => {
    if (open) setTab(defaultTab)
  }, [defaultTab, open])

  const [appConfig, setAppConfig] = useState<UpdateConfigBody | null>(null)
  const [providerCatalogs, setProviderCatalogs] = useState<LlmProviderCatalog[]>([])
  const [apiKeyDrafts, setApiKeyDrafts] = useState<Record<string, string>>({})
  const [dataPathDraft, setDataPathDraft] = useState('')
  const [httpConnectTimeoutDraft, setHttpConnectTimeoutDraft] = useState('')
  const [httpReadTimeoutDraft, setHttpReadTimeoutDraft] = useState('')
  const [httpMaxRetriesDraft, setHttpMaxRetriesDraft] = useState('')
  const [storageSettingsError, setStorageSettingsError] = useState<string | null>(null)
  const [isSavingStorageSettings, setIsSavingStorageSettings] = useState(false)
  const [engineCatalog, setEngineCatalog] = useState<GetEngineCatalog200 | null>(null)
  const [appVersion, setAppVersion] = useState<string>()
  const updater = useUpdater()

  useEffect(() => {
    if (!open) return
    void (async () => {
      try {
        const [config, catalog, engines] = await Promise.all([
          getConfig(),
          getLlmCatalog(),
          getEngineCatalog(),
        ])
        setAppConfig(config)
        setProviderCatalogs(catalog.providers)
        setEngineCatalog(engines)
      } catch {}
    })()
  }, [open])

  useEffect(() => {
    if (!open) return
    let cancelled = false
    void (async () => {
      try {
        const meta = await getMeta()
        if (cancelled) return
        setAppVersion(meta.version)
      } catch {
        return
      }
    })()
    return () => {
      cancelled = true
    }
  }, [open])

  useEffect(() => {
    if (!open || !isTauri()) return
    void updater.checkForUpdates()
  }, [open])

  useEffect(() => {
    if (!appConfig?.data) return
    setDataPathDraft(appConfig.data.path)
    setHttpConnectTimeoutDraft(
      String(appConfig.http?.connect_timeout ?? DEFAULT_HTTP_CONNECT_TIMEOUT),
    )
    setHttpReadTimeoutDraft(String(appConfig.http?.read_timeout ?? DEFAULT_HTTP_READ_TIMEOUT))
    setHttpMaxRetriesDraft(String(appConfig.http?.max_retries ?? DEFAULT_HTTP_MAX_RETRIES))
    setStorageSettingsError(null)
  }, [appConfig])

  const persistConfig = async (next: UpdateConfigBody) => {
    try {
      const saved = await updateConfig(next)
      const catalog = await getLlmCatalog()
      setAppConfig(saved)
      setProviderCatalogs(catalog.providers)
      queryClient.invalidateQueries({ queryKey: getGetLlmCatalogQueryKey() })
      return saved
    } catch {
      return null
    }
  }

  const upsertProvider = (id: string, updater: (p: ProviderConfig) => ProviderConfig) => {
    if (!appConfig) return
    const providers = [...(appConfig.providers ?? [])]
    const idx = providers.findIndex((p) => p.id === id)
    const current = idx >= 0 ? providers[idx] : { id }
    if (idx >= 0) providers[idx] = updater(current)
    else providers.push(updater(current))
    setAppConfig({ ...appConfig, providers })
  }

  const handleApplyStorageSettings = async () => {
    if (!appConfig) return
    const path = dataPathDraft.trim()
    if (!path) {
      setStorageSettingsError('Required')
      return
    }
    const connectTimeout = Number.parseInt(httpConnectTimeoutDraft.trim(), 10)
    if (!Number.isInteger(connectTimeout) || connectTimeout <= 0) {
      setStorageSettingsError('Invalid HTTP connect timeout')
      return
    }
    const readTimeout = Number.parseInt(httpReadTimeoutDraft.trim(), 10)
    if (!Number.isInteger(readTimeout) || readTimeout <= 0) {
      setStorageSettingsError('Invalid HTTP read timeout')
      return
    }
    const maxRetries = Number.parseInt(httpMaxRetriesDraft.trim(), 10)
    if (!Number.isInteger(maxRetries) || maxRetries < 0) {
      setStorageSettingsError('Invalid HTTP max retries')
      return
    }

    setIsSavingStorageSettings(true)
    setStorageSettingsError(null)
    const saved = await persistConfig({
      ...appConfig,
      data: { path },
      http: {
        connect_timeout: connectTimeout,
        read_timeout: readTimeout,
        max_retries: maxRetries,
      },
    })
    setIsSavingStorageSettings(false)
    if (!saved) {
      setStorageSettingsError('Failed')
      return
    }
    if (!isTauri()) {
      setStorageSettingsError('Restart manually')
      return
    }
    try {
      const { relaunch } = await import('@tauri-apps/plugin-process')
      await relaunch()
    } catch {
      setStorageSettingsError('Restart manually')
    }
  }

  const storageSettingsUnchanged =
    dataPathDraft.trim() === appConfig?.data?.path &&
    httpConnectTimeoutDraft.trim() ===
      String(appConfig?.http?.connect_timeout ?? DEFAULT_HTTP_CONNECT_TIMEOUT) &&
    httpReadTimeoutDraft.trim() ===
      String(appConfig?.http?.read_timeout ?? DEFAULT_HTTP_READ_TIMEOUT) &&
    httpMaxRetriesDraft.trim() === String(appConfig?.http?.max_retries ?? DEFAULT_HTTP_MAX_RETRIES)

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className='flex h-[600px] max-h-[85vh] w-[760px] max-w-[92vw] flex-col gap-0 overflow-hidden p-0'>
        <DialogTitle className='sr-only'>{t('settings.title')}</DialogTitle>
        <DialogDescription className='sr-only'>Settings</DialogDescription>

        <div className='flex h-full'>
          {/* Sidebar */}
          <nav className='flex w-[180px] shrink-0 flex-col gap-1 border-r border-border bg-muted/30 p-3'>
            <p className='mb-3 px-3 text-[10px] font-semibold tracking-widest text-muted-foreground uppercase'>
              {t('settings.title')}
            </p>
            {TABS.map(({ id, icon: Icon, labelKey }) => (
              <button
                key={id}
                onClick={() => setTab(id)}
                data-active={tab === id}
                className='flex items-center gap-3 rounded-lg px-3 py-2 text-sm text-muted-foreground transition hover:text-foreground data-[active=true]:bg-accent data-[active=true]:text-accent-foreground'
              >
                <Icon className='size-4 shrink-0' />
                {t(labelKey)}
              </button>
            ))}
          </nav>

          {/* Content */}
          <ScrollArea className='min-h-0 flex-1'>
            <div className='p-6'>
              {tab === 'appearance' && <AppearancePane />}
              {tab === 'engines' && engineCatalog && appConfig && (
                <EnginesPane
                  catalog={engineCatalog}
                  pipeline={appConfig.pipeline ?? {}}
                  onChange={(pipeline) => {
                    const next = { ...appConfig, pipeline }
                    setAppConfig(next)
                    void persistConfig(next)
                  }}
                />
              )}
              {tab === 'providers' && (
                <ProvidersPane
                  catalogs={providerCatalogs}
                  config={appConfig}
                  drafts={apiKeyDrafts}
                  onBaseUrlChange={(id, v) =>
                    upsertProvider(id, (p) => ({
                      ...p,
                      base_url: v || null,
                    }))
                  }
                  onBaseUrlBlur={() => appConfig && void persistConfig(appConfig)}
                  onApiKeyChange={(id, v) => setApiKeyDrafts((c) => ({ ...c, [id]: v }))}
                  onSaveKey={(id) => {
                    const key = apiKeyDrafts[id]?.trim()
                    if (!key || !appConfig) return
                    const providers = [...(appConfig.providers ?? [])]
                    const idx = providers.findIndex((p) => p.id === id)
                    const current = idx >= 0 ? providers[idx] : { id }
                    const updated = { ...current, api_key: key }
                    if (idx >= 0) providers[idx] = updated
                    else providers.push(updated)
                    void persistConfig({ ...appConfig, providers }).then(() =>
                      setApiKeyDrafts((c) => {
                        const n = { ...c }
                        delete n[id]
                        return n
                      }),
                    )
                  }}
                  onClearKey={(id) => {
                    if (!appConfig) return
                    const providers = [...(appConfig.providers ?? [])]
                    const idx = providers.findIndex((p) => p.id === id)
                    if (idx >= 0) providers[idx] = { ...providers[idx], api_key: null }
                    void persistConfig({ ...appConfig, providers }).then(() =>
                      setApiKeyDrafts((c) => {
                        const n = { ...c }
                        delete n[id]
                        return n
                      }),
                    )
                  }}
                />
              )}
              {tab === 'runtime' && (
                <StoragePane
                  dataPath={dataPathDraft}
                  httpConnectTimeout={httpConnectTimeoutDraft}
                  httpReadTimeout={httpReadTimeoutDraft}
                  httpMaxRetries={httpMaxRetriesDraft}
                  error={storageSettingsError}
                  saving={isSavingStorageSettings}
                  unchanged={storageSettingsUnchanged}
                  onPathChange={(v) => {
                    setDataPathDraft(v)
                    setStorageSettingsError(null)
                  }}
                  onHttpConnectTimeoutChange={(v) => {
                    setHttpConnectTimeoutDraft(v)
                    setStorageSettingsError(null)
                  }}
                  onHttpReadTimeoutChange={(v) => {
                    setHttpReadTimeoutDraft(v)
                    setStorageSettingsError(null)
                  }}
                  onHttpMaxRetriesChange={(v) => {
                    setHttpMaxRetriesDraft(v)
                    setStorageSettingsError(null)
                  }}
                  onApply={() => void handleApplyStorageSettings()}
                />
              )}
              {tab === 'keybinds' && <KeybindsPane />}
              {tab === 'about' && (
                <AboutPane
                  version={appVersion}
                  latestVersion={updater.latestVersion}
                  status={updater.status}
                  isInstallingUpdate={updater.isInstalling}
                  onInstallUpdate={() => void updater.installUpdate()}
                />
              )}
            </div>
          </ScrollArea>
        </div>
      </DialogContent>
    </Dialog>
  )
}

// ── Appearance ────────────────────────────────────────────────────

const THEMES = [
  { value: 'light', icon: SunIcon, labelKey: 'settings.themeLight' },
  { value: 'dark', icon: MoonIcon, labelKey: 'settings.themeDark' },
  { value: 'system', icon: MonitorIcon, labelKey: 'settings.themeSystem' },
] as const

function AppearancePane() {
  const { t, i18n } = useTranslation()
  const { theme, setTheme } = useTheme()
  const locales = useMemo(() => supportedLanguages, [])
  return (
    <div className='space-y-8'>
      <Section title={t('settings.theme')}>
        <div className='grid grid-cols-3 gap-3'>
          {THEMES.map(({ value, icon: Icon, labelKey }) => (
            <button
              key={value}
              onClick={() => setTheme(value)}
              data-active={theme === value}
              className='flex flex-col items-center gap-2 rounded-xl border border-border bg-card px-4 py-4 text-muted-foreground transition hover:border-foreground/30 data-[active=true]:border-primary data-[active=true]:text-foreground'
            >
              <Icon className='size-5' />
              <span className='text-xs font-medium'>{t(labelKey)}</span>
            </button>
          ))}
        </div>
      </Section>

      <Section title={t('settings.language')}>
        <Select value={i18n.language} onValueChange={(v) => i18n.changeLanguage(v)}>
          <SelectTrigger className='w-full'>
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            {locales.map((code) => (
              <SelectItem key={code} value={code}>
                {t(`menu.languages.${code}`)}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
      </Section>
    </div>
  )
}

// ── Engines ──────────────────────────────────────────────────────

function EnginesPane({
  catalog,
  pipeline,
  onChange,
}: {
  catalog: GetEngineCatalog200
  pipeline: import('@/lib/api/schemas').PipelineConfig
  onChange: (pipeline: import('@/lib/api/schemas').PipelineConfig) => void
}) {
  const { t } = useTranslation()

  const sections = [
    {
      label: t('settings.detector'),
      key: 'detector' as const,
      engines: catalog.detectors,
    },
    {
      label: t('settings.bubbleDetector'),
      key: 'bubble_detector' as const,
      engines: catalog.bubbleDetectors,
    },
    {
      label: t('settings.fontDetector'),
      key: 'font_detector' as const,
      engines: catalog.fontDetectors,
    },
    {
      label: t('settings.segmenter'),
      key: 'segmenter' as const,
      engines: catalog.segmenters,
    },
    { label: t('settings.ocr'), key: 'ocr' as const, engines: catalog.ocr },
    {
      label: t('settings.translator'),
      key: 'translator' as const,
      engines: catalog.translators,
    },
    {
      label: t('settings.inpainter'),
      key: 'inpainter' as const,
      engines: catalog.inpainters,
    },
    {
      label: t('settings.renderer'),
      key: 'renderer' as const,
      engines: catalog.renderers,
    },
  ]

  return (
    <div className='space-y-4'>
      <p className='text-xs text-muted-foreground'>{t('settings.enginesDescription')}</p>
      {sections.map(({ label, key, engines }) => (
        <div key={key} className='space-y-1.5'>
          <Label className='text-xs'>{label}</Label>
          <Select
            value={pipeline[key] ?? engines[0]?.id ?? ''}
            onValueChange={(v) => onChange({ ...pipeline, [key]: v })}
          >
            <SelectTrigger className='w-full'>
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              {engines.map((e) => (
                <SelectItem key={e.id} value={e.id}>
                  {e.name}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
        </div>
      ))}
    </div>
  )
}

// ── Providers ─────────────────────────────────────────────────────

function ProvidersPane({
  catalogs,
  config,
  drafts,
  onBaseUrlChange,
  onBaseUrlBlur,
  onApiKeyChange,
  onSaveKey,
  onClearKey,
}: {
  catalogs: LlmProviderCatalog[]
  config: UpdateConfigBody | null
  drafts: Record<string, string>
  onBaseUrlChange: (id: string, v: string) => void
  onBaseUrlBlur: () => void
  onApiKeyChange: (id: string, v: string) => void
  onSaveKey: (id: string) => void
  onClearKey: (id: string) => void
}) {
  const { t } = useTranslation()

  if (!catalogs.length)
    return (
      <p className='py-12 text-center text-sm text-muted-foreground'>
        {t('settings.loadingProviders')}
      </p>
    )

  return (
    <div className='space-y-6'>
      <Section title={t('settings.apiKeys')} description={t('settings.providersDescription')}>
        <Accordion type='multiple' className='-mx-1'>
          {catalogs.map((provider) => {
            const cfg = config?.providers?.find((p) => p.id === provider.id)
            const draft = drafts[provider.id] ?? ''
            const hasDraft = draft.trim().length > 0
            const statusColor =
              provider.status === 'ready'
                ? 'bg-green-500'
                : provider.status === 'missing_configuration'
                  ? 'bg-amber-400'
                  : provider.status === 'discovery_failed'
                    ? 'bg-red-500'
                    : 'bg-muted-foreground'

            return (
              <AccordionItem key={provider.id} value={provider.id} className='border-border'>
                <AccordionTrigger className='px-1 py-3 hover:no-underline'>
                  <div className='flex items-center gap-2.5'>
                    <span className={`size-2 shrink-0 rounded-full ${statusColor}`} />
                    <span className='text-sm font-medium'>{provider.name}</span>
                  </div>
                </AccordionTrigger>
                <AccordionContent className='space-y-4 px-1 pt-1 pb-4'>
                  {provider.error && (
                    <p className='text-xs text-muted-foreground'>{provider.error}</p>
                  )}

                  {provider.requiresBaseUrl && (
                    <div className='space-y-1.5'>
                      <Label className='text-xs'>{t('settings.localLlmBaseUrl')}</Label>
                      <Input
                        type='url'
                        value={cfg?.base_url ?? ''}
                        onChange={(e) => onBaseUrlChange(provider.id, e.target.value)}
                        onBlur={onBaseUrlBlur}
                        placeholder='https://api.example.com/v1'
                      />
                    </div>
                  )}

                  <div className='space-y-1.5'>
                    <Label className='text-xs'>{t('settings.apiKey')}</Label>
                    <div className='flex gap-2'>
                      <Input
                        type='password'
                        value={draft}
                        onChange={(e) => onApiKeyChange(provider.id, e.target.value)}
                        onKeyDown={(e) => {
                          if (e.key === 'Enter' && hasDraft) onSaveKey(provider.id)
                        }}
                        placeholder={
                          cfg?.api_key === '[REDACTED]'
                            ? t('settings.apiKeyPlaceholderStored')
                            : t('settings.apiKeyPlaceholderEmpty')
                        }
                        className='[&::-ms-reveal]:hidden'
                      />
                      {hasDraft ? (
                        <Button size='sm' onClick={() => onSaveKey(provider.id)}>
                          {t('settings.apiKeySave')}
                        </Button>
                      ) : cfg?.api_key === '[REDACTED]' ? (
                        <Button
                          variant='destructive'
                          size='sm'
                          onClick={() => onClearKey(provider.id)}
                        >
                          {t('settings.apiKeyClear')}
                        </Button>
                      ) : null}
                    </div>
                  </div>
                </AccordionContent>
              </AccordionItem>
            )
          })}
        </Accordion>
      </Section>
    </div>
  )
}

// ── Keybinds ──────────────────────────────────────────────────────

const SHORTCUT_ITEMS = [
  { key: 'select', labelKey: 'toolRail.select' },
  { key: 'block', labelKey: 'toolRail.block' },
  { key: 'brush', labelKey: 'toolRail.brush' },
  { key: 'eraser', labelKey: 'toolRail.eraser' },
  { key: 'repairBrush', labelKey: 'toolRail.repairBrush' },
  {
    key: 'increaseBrushSize',
    labelKey: 'settings.shortcutIncreaseBrushSize',
  },
  {
    key: 'decreaseBrushSize',
    labelKey: 'settings.shortcutDecreaseBrushSize',
  },
] as const

function KeybindsPane() {
  const { t } = useTranslation()
  const shortcuts = usePreferencesStore((state) => state.shortcuts)
  const setShortcuts = usePreferencesStore((state) => state.setShortcuts)
  const resetShortcutsStore = usePreferencesStore((state) => state.resetShortcuts)
  const [pendingShortcuts, setPendingShortcuts] = useState(shortcuts)
  const [recordingKey, setRecordingKey] = useState<string | null>(null)
  const [error, setError] = useState<string | null>(null)
  const [isSaved, setIsSaved] = useState(false)
  const isMac = useMemo(() => getPlatform() === 'mac', [])

  // Optimized conflict detection
  const conflictCounts = useMemo(() => {
    const counts = new Map<string, number>()
    Object.values(pendingShortcuts).forEach((val) => {
      counts.set(val, (counts.get(val) || 0) + 1)
    })
    return counts
  }, [pendingShortcuts])

  const isDirty = useMemo(
    () => !areShortcutsEqual(shortcuts, pendingShortcuts),
    [shortcuts, pendingShortcuts],
  )

  // Sync from store if it changes (e.g. externally via Reset)
  useEffect(() => {
    setPendingShortcuts(shortcuts)
  }, [shortcuts])

  useEffect(() => {
    if (!recordingKey) {
      setError(null)
      return
    }

    const handleKeyDown = (e: KeyboardEvent) => {
      e.preventDefault()
      e.stopPropagation()

      // Early exit for modifier-only events
      if (isModifierKey(e.key)) {
        return
      }

      // Allow Escape to cancel recording
      if (e.key === 'Escape') {
        setRecordingKey(null)
        return
      }

      // Block system/function keys
      if (isKeyBlocked(e.key)) {
        setError(t('settings.shortcutInvalid'))
        return
      }

      const shortcut = formatShortcut(e, isMac)
      if (!shortcut) return

      // Conflict detection (now non-blocking)
      const existingAction = Object.entries(pendingShortcuts).find(
        ([action, val]) => val === shortcut && action !== recordingKey,
      )

      if (existingAction) {
        // Just a hint, we still allow it
        setError(t('settings.shortcutConflict'))
      }

      setPendingShortcuts((prev) => ({ ...prev, [recordingKey]: shortcut }))
      setRecordingKey(null)
      setIsSaved(false)
    }

    const handleClickOutside = () => {
      setRecordingKey(null)
    }

    window.addEventListener('keydown', handleKeyDown, { capture: true })
    window.addEventListener('click', handleClickOutside, { capture: true })
    return () => {
      window.removeEventListener('keydown', handleKeyDown, { capture: true })
      window.removeEventListener('click', handleClickOutside, {
        capture: true,
      })
    }
  }, [recordingKey, pendingShortcuts, t, isMac])

  const [resetConfirmOpen, setResetConfirmOpen] = useState(false)

  const saveTimeoutRef = useRef<ReturnType<typeof setTimeout> | null>(null)
  useEffect(() => {
    return () => {
      if (saveTimeoutRef.current) {
        clearTimeout(saveTimeoutRef.current)
      }
    }
  }, [])

  const handleSave = () => {
    setShortcuts(pendingShortcuts)
    setIsSaved(true)

    if (saveTimeoutRef.current) {
      clearTimeout(saveTimeoutRef.current)
    }

    saveTimeoutRef.current = setTimeout(() => {
      setIsSaved(false)
      saveTimeoutRef.current = null
    }, 2000)
  }

  const handleReset = () => {
    setResetConfirmOpen(true)
  }

  const handleConfirmReset = () => {
    resetShortcutsStore()
    setResetConfirmOpen(false)
  }

  return (
    <div className='flex h-full flex-col gap-6'>
      <div className='grow space-y-6 overflow-y-auto pr-2'>
        <Section title={t('settings.keybinds')} description={t('settings.keybindsDescription')}>
          <div className='divide-y divide-border overflow-hidden rounded-xl border border-border bg-card'>
            {SHORTCUT_ITEMS.map((item) => {
              const currentVal = pendingShortcuts[item.key]
              const hasConflict = (conflictCounts.get(currentVal) || 0) > 1

              return (
                <div key={item.key} className='flex items-center justify-between px-4 py-3'>
                  <div className='flex items-center gap-2'>
                    <span className='text-sm'>{t(item.labelKey)}</span>
                    {hasConflict && (
                      <div title={t('settings.shortcutConflict')}>
                        <AlertTriangleIcon className='size-3.5 text-amber-500' />
                      </div>
                    )}
                  </div>
                  <Button
                    variant={recordingKey === item.key ? 'default' : 'outline'}
                    size='sm'
                    onClick={(e) => {
                      e.stopPropagation()
                      setRecordingKey(item.key)
                    }}
                    className='h-8 min-w-16 font-mono uppercase'
                  >
                    {recordingKey === item.key
                      ? error || t('settings.shortcutPressKey')
                      : currentVal || 'NONE'}
                  </Button>
                </div>
              )
            })}
          </div>
        </Section>
      </div>

      <div className='flex items-center justify-between border-t border-border pt-4'>
        <Button
          variant='ghost'
          size='sm'
          className='gap-2 text-muted-foreground hover:text-foreground'
          onClick={handleReset}
        >
          <RotateCcwIcon className='size-4' />
          {t('settings.shortcutReset')}
        </Button>
        <div className='flex items-center gap-2'>
          <Button
            variant='default'
            size='sm'
            disabled={!isDirty || isSaved}
            onClick={handleSave}
            className='min-w-32 gap-2'
          >
            {isSaved ? (
              <>
                <CheckCircleIcon className='size-4' />
                {t('common.saved')}
              </>
            ) : (
              <>
                <SaveIcon className='size-4' />
                {t('common.save')}
              </>
            )}
          </Button>
        </div>
      </div>
      <AlertDialog open={resetConfirmOpen} onOpenChange={setResetConfirmOpen}>
        <AlertDialogContent>
          <AlertDialogTitle>{t('settings.shortcutReset')}</AlertDialogTitle>
          <AlertDialogDescription>{t('settings.shortcutResetDescription')}</AlertDialogDescription>
          <div className='flex justify-end gap-2'>
            <AlertDialogCancel>{t('common.cancel')}</AlertDialogCancel>
            <AlertDialogAction onClick={handleConfirmReset}>
              {t('common.confirm')}
            </AlertDialogAction>
          </div>
        </AlertDialogContent>
      </AlertDialog>
    </div>
  )
}

// ── Storage ───────────────────────────────────────────────────────

function StoragePane({
  dataPath,
  httpConnectTimeout,
  httpReadTimeout,
  httpMaxRetries,
  error,
  saving,
  unchanged,
  onPathChange,
  onHttpConnectTimeoutChange,
  onHttpReadTimeoutChange,
  onHttpMaxRetriesChange,
  onApply,
}: {
  dataPath: string
  httpConnectTimeout: string
  httpReadTimeout: string
  httpMaxRetries: string
  error: string | null
  saving: boolean
  unchanged: boolean
  onPathChange: (v: string) => void
  onHttpConnectTimeoutChange: (v: string) => void
  onHttpReadTimeoutChange: (v: string) => void
  onHttpMaxRetriesChange: (v: string) => void
  onApply: () => void
}) {
  const { t } = useTranslation()
  const [confirmOpen, setConfirmOpen] = useState(false)

  return (
    <>
      <Section title={t('settings.runtime')} description={t('settings.runtimeDescription')}>
        <div className='space-y-1.5'>
          <Label className='text-xs'>{t('settings.dataPath')}</Label>
          <Input type='text' value={dataPath} onChange={(e) => onPathChange(e.target.value)} />
          <p className='text-xs leading-relaxed text-muted-foreground'>
            {t('settings.dataPathDescription')}
          </p>
        </div>

        <div className='grid gap-4 md:grid-cols-2'>
          <div className='space-y-1.5'>
            <Label className='text-xs'>{t('settings.httpConnectTimeout')}</Label>
            <Input
              type='number'
              min='1'
              step='1'
              inputMode='numeric'
              value={httpConnectTimeout}
              onChange={(e) => onHttpConnectTimeoutChange(e.target.value)}
            />
            <p className='text-xs leading-relaxed text-muted-foreground'>
              {t('settings.httpConnectTimeoutDescription')}
            </p>
          </div>

          <div className='space-y-1.5'>
            <Label className='text-xs'>{t('settings.httpReadTimeout')}</Label>
            <Input
              type='number'
              min='1'
              step='1'
              inputMode='numeric'
              value={httpReadTimeout}
              onChange={(e) => onHttpReadTimeoutChange(e.target.value)}
            />
            <p className='text-xs leading-relaxed text-muted-foreground'>
              {t('settings.httpReadTimeoutDescription')}
            </p>
          </div>
        </div>

        <div className='space-y-1.5'>
          <Label className='text-xs'>{t('settings.httpMaxRetries')}</Label>
          <Input
            type='number'
            min='0'
            step='1'
            inputMode='numeric'
            value={httpMaxRetries}
            onChange={(e) => onHttpMaxRetriesChange(e.target.value)}
          />
          <p className='text-xs leading-relaxed text-muted-foreground'>
            {t('settings.httpMaxRetriesDescription')}
          </p>
        </div>

        {error && <p className='text-xs text-destructive'>{error}</p>}
        <div className='flex justify-end pt-1'>
          <Button
            onClick={() => setConfirmOpen(true)}
            disabled={!dataPath.trim() || saving || unchanged}
          >
            {saving ? t('settings.restartApplying') : t('settings.restartApply')}
          </Button>
        </div>
      </Section>

      <AlertDialog open={confirmOpen} onOpenChange={setConfirmOpen}>
        <AlertDialogContent>
          <AlertDialogTitle>{t('settings.restartApply')}</AlertDialogTitle>
          <AlertDialogDescription>
            {t('settings.restartRequiredDescription')}
          </AlertDialogDescription>
          <div className='flex justify-end gap-2'>
            <AlertDialogCancel>{t('common.cancel')}</AlertDialogCancel>
            <AlertDialogAction
              onClick={() => {
                setConfirmOpen(false)
                onApply()
              }}
            >
              {t('settings.restartApply')}
            </AlertDialogAction>
          </div>
        </AlertDialogContent>
      </AlertDialog>
    </>
  )
}

// ── About ─────────────────────────────────────────────────────────

function AboutPane({
  version,
  latestVersion,
  status,
  isInstallingUpdate,
  onInstallUpdate,
}: {
  version?: string
  latestVersion?: string
  status: UpdaterStatus
  isInstallingUpdate: boolean
  onInstallUpdate: () => void
}) {
  const { t } = useTranslation()

  return (
    <div className='flex h-full flex-col items-center justify-center gap-5 py-8'>
      <img src='/icon-large.png' alt='Koharu' className='size-20' draggable={false} />
      <div className='text-center'>
        <h2 className='text-lg font-bold tracking-wide text-foreground'>Koharu</h2>
        <p className='mt-1 text-sm text-muted-foreground'>{t('settings.aboutTagline')}</p>
      </div>

      <div className='w-full max-w-sm rounded-xl border border-border bg-card p-4'>
        <div className='space-y-3 text-sm'>
          <InfoRow label={t('settings.aboutVersion')}>
            <div className='flex flex-col items-end gap-0.5'>
              <span className='font-mono text-xs font-medium'>{version || '...'}</span>
              {status === 'loading' && (
                <LoaderIcon className='size-3.5 animate-spin text-muted-foreground' />
              )}
              {status === 'latest' && (
                <span className='flex items-center gap-1 text-xs text-green-500'>
                  <CheckCircleIcon className='size-3.5' />
                  {t('settings.aboutLatest')}
                </span>
              )}
              {status === 'outdated' && (
                <Button
                  variant='link'
                  size='xs'
                  onClick={onInstallUpdate}
                  disabled={isInstallingUpdate}
                  className='h-auto gap-1 p-0 text-amber-500'
                >
                  {isInstallingUpdate ? (
                    <LoaderIcon className='size-3.5 animate-spin' />
                  ) : (
                    <AlertCircleIcon className='size-3.5' />
                  )}
                  {t('settings.aboutUpdate', { version: latestVersion })}
                </Button>
              )}
            </div>
          </InfoRow>
          <InfoRow label={t('settings.aboutAuthor')}>
            <Button
              variant='link'
              size='xs'
              onClick={() => void openExternalUrl('https://github.com/mayocream')}
            >
              Mayo
            </Button>
          </InfoRow>
          <InfoRow label={t('settings.aboutRepository')}>
            <Button
              variant='link'
              size='xs'
              onClick={() => void openExternalUrl(`https://github.com/${GITHUB_REPO}`)}
            >
              GitHub
            </Button>
          </InfoRow>
        </div>
      </div>
    </div>
  )
}

// ── Shared ────────────────────────────────────────────────────────

function Section({
  title,
  description,
  children,
}: {
  title: string
  description?: string
  children: ReactNode
}) {
  return (
    <div className='space-y-3'>
      <div>
        <h3 className='text-sm font-semibold text-foreground'>{title}</h3>
        {description && (
          <p className='mt-0.5 text-xs leading-relaxed text-muted-foreground'>{description}</p>
        )}
      </div>
      {children}
    </div>
  )
}

function InfoRow({ label, children }: { label: string; children: ReactNode }) {
  return (
    <div className='flex items-center justify-between'>
      <span className='text-muted-foreground'>{label}</span>
      <div className='flex items-center'>{children}</div>
    </div>
  )
}
