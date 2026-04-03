'use client'

import { useEffect, useMemo, useState, type ReactNode } from 'react'
import { useQueryClient } from '@tanstack/react-query'
import { useTheme } from 'next-themes'
import { useTranslation } from 'react-i18next'
import {
  SunIcon,
  MoonIcon,
  MonitorIcon,
  CheckCircleIcon,
  AlertCircleIcon,
  LoaderIcon,
  PaletteIcon,
  KeyIcon,
  FolderIcon,
  InfoIcon,
  CpuIcon,
} from 'lucide-react'
import {
  Dialog,
  DialogContent,
  DialogTitle,
  DialogDescription,
} from '@/components/ui/dialog'
import {
  Accordion,
  AccordionItem,
  AccordionTrigger,
  AccordionContent,
} from '@/components/ui/accordion'
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select'
import { ScrollArea } from '@/components/ui/scroll-area'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import {
  AlertDialog,
  AlertDialogAction,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogTitle,
} from '@/components/ui/alert-dialog'
import { isTauri } from '@/lib/backend'
import {
  getConfig,
  getEngineCatalog,
  getMeta,
  updateConfig,
} from '@/lib/api/system/system'
import { getLlmCatalog, getGetLlmCatalogQueryKey } from '@/lib/api/llm/llm'
import { usePreferencesStore } from '@/lib/stores/preferencesStore'
import { supportedLanguages } from '@/lib/i18n'
import type {
  UpdateConfigBody,
  ProviderConfig,
  LlmProviderCatalog,
  GetEngineCatalog200,
} from '@/lib/api/schemas'

const GITHUB_REPO = 'mayocream/koharu'

type VersionStatus = 'loading' | 'latest' | 'outdated' | 'error'

const TABS = [
  { id: 'appearance', icon: PaletteIcon, labelKey: 'settings.appearance' },
  { id: 'engines', icon: CpuIcon, labelKey: 'settings.engines' },
  { id: 'providers', icon: KeyIcon, labelKey: 'settings.apiKeys' },
  { id: 'storage', icon: FolderIcon, labelKey: 'settings.storage' },
  { id: 'about', icon: InfoIcon, labelKey: 'settings.about' },
] as const

export type TabId = (typeof TABS)[number]['id']

type SettingsDialogProps = {
  open: boolean
  onOpenChange: (open: boolean) => void
  defaultTab?: TabId
}

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
  const [providerCatalogs, setProviderCatalogs] = useState<
    LlmProviderCatalog[]
  >([])
  const [apiKeyDrafts, setApiKeyDrafts] = useState<Record<string, string>>({})
  const [dataPathDraft, setDataPathDraft] = useState('')
  const [dataPathError, setDataPathError] = useState<string | null>(null)
  const [isSavingDataPath, setIsSavingDataPath] = useState(false)
  const [engineCatalog, setEngineCatalog] =
    useState<GetEngineCatalog200 | null>(null)
  const [appVersion, setAppVersion] = useState<string>()
  const [latestVersion, setLatestVersion] = useState<string>()
  const [versionStatus, setVersionStatus] = useState<VersionStatus>('loading')

  useEffect(() => {
    if (!open) return
    void (async () => {
      try {
        const [config, catalog, engines] = await Promise.all([
          getConfig() as unknown as Promise<UpdateConfigBody>,
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
    void (async () => {
      try {
        const meta = await getMeta()
        setAppVersion(meta.version)
        const res = await fetch(
          `https://api.github.com/repos/${GITHUB_REPO}/releases/latest`,
        )
        if (res.ok) {
          const data = await res.json()
          const latest = data.tag_name?.replace(/^v/, '') || data.name
          setLatestVersion(latest)
          setVersionStatus(meta.version === latest ? 'latest' : 'outdated')
        } else setVersionStatus('error')
      } catch {
        setVersionStatus('error')
      }
    })()
  }, [open])

  useEffect(() => {
    if (appConfig?.data) {
      setDataPathDraft(appConfig.data.path)
      setDataPathError(null)
    }
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

  const upsertProvider = (
    id: string,
    updater: (p: ProviderConfig) => ProviderConfig,
  ) => {
    if (!appConfig) return
    const providers = [...(appConfig.providers ?? [])]
    const idx = providers.findIndex((p) => p.id === id)
    const current = idx >= 0 ? providers[idx] : { id }
    if (idx >= 0) providers[idx] = updater(current)
    else providers.push(updater(current))
    setAppConfig({ ...appConfig, providers })
  }

  const handleApplyDataPath = async () => {
    if (!appConfig) return
    const path = dataPathDraft.trim()
    if (!path) {
      setDataPathError('Required')
      return
    }
    if (path === appConfig.data?.path) return
    setIsSavingDataPath(true)
    setDataPathError(null)
    const saved = await persistConfig({ ...appConfig, data: { path } })
    setIsSavingDataPath(false)
    if (!saved) {
      setDataPathError('Failed')
      return
    }
    if (isTauri()) {
      try {
        const { relaunch } = await import('@tauri-apps/plugin-process')
        await relaunch()
      } catch {
        setDataPathError('Restart manually')
      }
    }
  }

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className='flex h-[600px] max-h-[85vh] w-[760px] max-w-[92vw] flex-col gap-0 overflow-hidden p-0'>
        <DialogTitle className='sr-only'>{t('settings.title')}</DialogTitle>
        <DialogDescription className='sr-only'>Settings</DialogDescription>

        <div className='flex h-full'>
          {/* Sidebar */}
          <nav className='bg-muted/30 border-border flex w-[180px] shrink-0 flex-col gap-1 border-r p-3'>
            <p className='text-muted-foreground mb-3 px-3 text-[10px] font-semibold tracking-widest uppercase'>
              {t('settings.title')}
            </p>
            {TABS.map(({ id, icon: Icon, labelKey }) => (
              <button
                key={id}
                onClick={() => setTab(id)}
                data-active={tab === id}
                className='text-muted-foreground hover:text-foreground data-[active=true]:bg-accent data-[active=true]:text-accent-foreground flex items-center gap-3 rounded-lg px-3 py-2 text-sm transition'
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
                  onBaseUrlBlur={() =>
                    appConfig && void persistConfig(appConfig)
                  }
                  onApiKeyChange={(id, v) =>
                    setApiKeyDrafts((c) => ({ ...c, [id]: v }))
                  }
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
                    if (idx >= 0)
                      providers[idx] = { ...providers[idx], api_key: null }
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
              {tab === 'storage' && (
                <StoragePane
                  dataPath={dataPathDraft}
                  error={dataPathError}
                  saving={isSavingDataPath}
                  unchanged={dataPathDraft.trim() === appConfig?.data?.path}
                  onPathChange={(v) => {
                    setDataPathDraft(v)
                    setDataPathError(null)
                  }}
                  onApply={() => void handleApplyDataPath()}
                />
              )}
              {tab === 'about' && (
                <AboutPane
                  version={appVersion}
                  latestVersion={latestVersion}
                  status={versionStatus}
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
  const fontFamily = usePreferencesStore((s) => s.fontFamily)
  const setFontFamily = usePreferencesStore((s) => s.setFontFamily)

  return (
    <div className='space-y-8'>
      <Section title={t('settings.theme')}>
        <div className='grid grid-cols-3 gap-3'>
          {THEMES.map(({ value, icon: Icon, labelKey }) => (
            <button
              key={value}
              onClick={() => setTheme(value)}
              data-active={theme === value}
              className='border-border bg-card text-muted-foreground hover:border-foreground/30 data-[active=true]:border-primary data-[active=true]:text-foreground flex flex-col items-center gap-2 rounded-xl border px-4 py-4 transition'
            >
              <Icon className='size-5' />
              <span className='text-xs font-medium'>{t(labelKey)}</span>
            </button>
          ))}
        </div>
      </Section>

      <Section title={t('settings.language')}>
        <Select
          value={i18n.language}
          onValueChange={(v) => i18n.changeLanguage(v)}
        >
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

      <Section
        title={t('settings.renderingFont')}
        description={t('settings.renderingFontDescription')}
      >
        <Input
          type='text'
          value={fontFamily ?? ''}
          onChange={(e) => setFontFamily(e.target.value || undefined)}
          placeholder='e.g. Noto Sans CJK'
        />
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
      <p className='text-muted-foreground text-xs'>
        {t('settings.enginesDescription')}
      </p>
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
      <p className='text-muted-foreground py-12 text-center text-sm'>
        {t('settings.loadingProviders')}
      </p>
    )

  return (
    <div className='space-y-6'>
      <Section
        title={t('settings.apiKeys')}
        description={t('settings.providersDescription')}
      >
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
              <AccordionItem
                key={provider.id}
                value={provider.id}
                className='border-border'
              >
                <AccordionTrigger className='px-1 py-3 hover:no-underline'>
                  <div className='flex items-center gap-2.5'>
                    <span
                      className={`size-2 shrink-0 rounded-full ${statusColor}`}
                    />
                    <span className='text-sm font-medium'>{provider.name}</span>
                  </div>
                </AccordionTrigger>
                <AccordionContent className='space-y-4 px-1 pt-1 pb-4'>
                  {provider.error && (
                    <p className='text-muted-foreground text-xs'>
                      {provider.error}
                    </p>
                  )}

                  {provider.requiresBaseUrl && (
                    <div className='space-y-1.5'>
                      <Label className='text-xs'>
                        {t('settings.localLlmBaseUrl')}
                      </Label>
                      <Input
                        type='url'
                        value={cfg?.base_url ?? ''}
                        onChange={(e) =>
                          onBaseUrlChange(provider.id, e.target.value)
                        }
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
                        onChange={(e) =>
                          onApiKeyChange(provider.id, e.target.value)
                        }
                        onKeyDown={(e) => {
                          if (e.key === 'Enter' && hasDraft)
                            onSaveKey(provider.id)
                        }}
                        placeholder={
                          cfg?.api_key === '[REDACTED]'
                            ? t('settings.apiKeyPlaceholderStored')
                            : t('settings.apiKeyPlaceholderEmpty')
                        }
                        className='[&::-ms-reveal]:hidden'
                      />
                      {hasDraft ? (
                        <Button
                          size='sm'
                          onClick={() => onSaveKey(provider.id)}
                        >
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

// ── Storage ───────────────────────────────────────────────────────

function StoragePane({
  dataPath,
  error,
  saving,
  unchanged,
  onPathChange,
  onApply,
}: {
  dataPath: string
  error: string | null
  saving: boolean
  unchanged: boolean
  onPathChange: (v: string) => void
  onApply: () => void
}) {
  const { t } = useTranslation()
  const [confirmOpen, setConfirmOpen] = useState(false)

  return (
    <>
      <Section
        title={t('settings.dataPath')}
        description={t('settings.dataPathDescription')}
      >
        <Input
          type='text'
          value={dataPath}
          onChange={(e) => onPathChange(e.target.value)}
        />
        {error && <p className='text-destructive text-xs'>{error}</p>}
        <div className='flex justify-end pt-1'>
          <Button
            onClick={() => setConfirmOpen(true)}
            disabled={!dataPath.trim() || saving || unchanged}
          >
            {saving
              ? t('settings.dataPathApplying')
              : t('settings.dataPathApply')}
          </Button>
        </div>
      </Section>

      <AlertDialog open={confirmOpen} onOpenChange={setConfirmOpen}>
        <AlertDialogContent>
          <AlertDialogTitle>{t('settings.dataPathApply')}</AlertDialogTitle>
          <AlertDialogDescription>
            {t('settings.dataPathDescription')}
          </AlertDialogDescription>
          <div className='flex justify-end gap-2'>
            <AlertDialogCancel>{t('common.cancel')}</AlertDialogCancel>
            <AlertDialogAction
              onClick={() => {
                setConfirmOpen(false)
                onApply()
              }}
            >
              {t('settings.dataPathApply')}
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
}: {
  version?: string
  latestVersion?: string
  status: VersionStatus
}) {
  const { t } = useTranslation()
  const open = (url: string) =>
    window.open(url, '_blank', 'noopener,noreferrer')

  return (
    <div className='flex h-full flex-col items-center justify-center gap-5 py-8'>
      <img
        src='/icon-large.png'
        alt='Koharu'
        className='size-20'
        draggable={false}
      />
      <div className='text-center'>
        <h2 className='text-foreground text-lg font-bold tracking-wide'>
          Koharu
        </h2>
        <p className='text-muted-foreground mt-1 text-sm'>
          {t('settings.aboutTagline')}
        </p>
      </div>

      <div className='bg-card border-border w-full max-w-sm rounded-xl border p-4'>
        <div className='space-y-3 text-sm'>
          <InfoRow label={t('settings.aboutVersion')}>
            <div className='flex flex-col items-end gap-0.5'>
              <span className='font-mono text-xs font-medium'>
                {version || '...'}
              </span>
              {status === 'loading' && (
                <LoaderIcon className='text-muted-foreground size-3.5 animate-spin' />
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
                  onClick={() =>
                    open(`https://github.com/${GITHUB_REPO}/releases/latest`)
                  }
                  className='h-auto gap-1 p-0 text-amber-500'
                >
                  <AlertCircleIcon className='size-3.5' />
                  {t('settings.aboutUpdate', { version: latestVersion })}
                </Button>
              )}
            </div>
          </InfoRow>
          <InfoRow label={t('settings.aboutAuthor')}>
            <Button
              variant='link'
              size='xs'
              onClick={() => open('https://github.com/mayocream')}
            >
              Mayo
            </Button>
          </InfoRow>
          <InfoRow label={t('settings.aboutRepository')}>
            <Button
              variant='link'
              size='xs'
              onClick={() => open(`https://github.com/${GITHUB_REPO}`)}
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
        <h3 className='text-foreground text-sm font-semibold'>{title}</h3>
        {description && (
          <p className='text-muted-foreground mt-0.5 text-xs leading-relaxed'>
            {description}
          </p>
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
