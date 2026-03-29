'use client'

import { useEffect, useMemo, useRef, useState, type ReactNode } from 'react'
import { Loader2 } from 'lucide-react'
import { useTranslation } from 'react-i18next'
import { api } from '@/lib/api'
import type { Config } from '@/lib/protocol'
import { useDownloadStore } from '@/lib/downloads'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select'

const cloneConfig = (config: Config): Config => ({
  ...config,
  pypiMirror: { ...config.pypiMirror },
  githubMirror: { ...config.githubMirror },
})

type InitializeState = 'idle' | 'running' | 'failed' | 'done'

export function OnboardingScreen() {
  const { i18n } = useTranslation()
  const downloads = useDownloadStore((state) => state.downloads)
  const clearDownloads = useDownloadStore((state) => state.clear)
  const [draft, setDraft] = useState<Config | null>(null)
  const [step, setStep] = useState(0)
  const [loadVersion, setLoadVersion] = useState(0)
  const [loadingConfig, setLoadingConfig] = useState(true)
  const [loadingError, setLoadingError] = useState<string | null>(null)
  const [saving, setSaving] = useState(false)
  const [initializeState, setInitializeState] =
    useState<InitializeState>('idle')
  const [initializeError, setInitializeError] = useState<string | null>(null)
  const [initializeRequestId, setInitializeRequestId] = useState(0)
  const draftRef = useRef<Config | null>(null)
  const lastInitializeRequestIdRef = useRef(0)
  const lastActiveItemRef = useRef<string>('')

  const locales = useMemo(
    () => Object.keys(i18n.options.resources || {}),
    [i18n.options.resources],
  )
  const totalSteps = 4
  const previewLanguage =
    draft?.language || i18n.resolvedLanguage || i18n.language
  const t = useMemo(
    () => i18n.getFixedT(previewLanguage),
    [i18n, previewLanguage],
  )

  useEffect(() => {
    draftRef.current = draft
  }, [draft])

  useEffect(() => {
    let cancelled = false
    setLoadingConfig(true)
    setLoadingError(null)

    void api
      .getConfig()
      .then((config) => {
        if (cancelled) return
        setDraft(cloneConfig(config))
        if (config.language && i18n.language !== config.language) {
          void i18n.changeLanguage(config.language)
        }
      })
      .catch((error) => {
        if (cancelled) return
        setLoadingError(String(error))
      })
      .finally(() => {
        if (!cancelled) {
          setLoadingConfig(false)
        }
      })

    return () => {
      cancelled = true
    }
  }, [i18n, loadVersion])

  const currentDownload = useMemo(() => {
    const active = Array.from(downloads.values()).filter(
      (entry) => entry.status === 'started' || entry.status === 'downloading',
    )

    if (active.length > 0) {
      const current = active[active.length - 1]
      lastActiveItemRef.current = current.label || current.filename
    }

    return lastActiveItemRef.current
  }, [downloads])

  useEffect(() => {
    if (step !== 3 || initializeRequestId === 0) {
      return
    }

    if (lastInitializeRequestIdRef.current === initializeRequestId) {
      return
    }
    lastInitializeRequestIdRef.current = initializeRequestId

    const requestConfig = draftRef.current
    if (!requestConfig) {
      return
    }

    let cancelled = false
    clearDownloads()
    lastActiveItemRef.current = ''
    setSaving(true)
    setInitializeState('running')
    setInitializeError(null)

    void (async () => {
      try {
        const saved = await api.updateConfig(requestConfig)
        if (cancelled) return
        setDraft(cloneConfig(saved))
        await i18n.changeLanguage(saved.language)
        await api.initialize()
        if (cancelled) return
        setInitializeState('done')
      } catch (error) {
        if (cancelled) return
        setInitializeState('failed')
        setInitializeError(String(error))
      } finally {
        if (!cancelled) {
          setSaving(false)
        }
      }
    })()

    return () => {
      cancelled = true
    }
  }, [clearDownloads, i18n, initializeRequestId, step])

  const canProceed = useMemo(() => {
    if (!draft) return false

    switch (step) {
      case 0:
        return Boolean(draft.language)
      case 1:
        return (
          Boolean(draft.runtimePath.trim()) && Boolean(draft.modelsPath.trim())
        )
      case 2:
        return [draft.pypiMirror, draft.githubMirror].every(
          (mirror) =>
            mirror.kind === 'official' || Boolean(mirror.customBaseUrl?.trim()),
        )
      default:
        return true
    }
  }, [draft, step])

  const handleConfigChange = <K extends keyof Config>(
    key: K,
    value: Config[K],
  ) => {
    setDraft((current) => (current ? { ...current, [key]: value } : current))
  }

  const handleNext = () => {
    if (step >= totalSteps - 1 || !canProceed) return
    const nextStep = step + 1
    setStep(nextStep)
    if (nextStep === 3) {
      setInitializeRequestId((current) => current + 1)
    }
  }

  const handleBack = () => {
    if (step === 0) return
    setStep((current) => current - 1)
  }

  if (loadingConfig || !draft) {
    if (!loadingConfig && !draft) {
      return (
        <div className='bg-background flex h-screen flex-col overflow-hidden'>
          <header
            data-tauri-drag-region
            className='flex h-4 shrink-0 items-center justify-end px-2.5'
          >
            <div className='bg-border h-1 w-2 rounded-full' />
          </header>
          <main className='flex flex-1 flex-col px-2.5 pt-0.5 pb-1.5'>
            <div className='flex min-h-0 flex-1 items-center justify-center'>
              <div className='w-full max-w-[220px] text-center'>
                <div className='text-destructive line-clamp-3 text-[11px]'>
                  {loadingError ?? 'Failed to load config.'}
                </div>
              </div>
            </div>
            <footer className='border-border mt-1.5 flex h-8 shrink-0 items-center justify-end border-t pt-1.5'>
              <Button
                size='xs'
                onClick={() => setLoadVersion((current) => current + 1)}
              >
                {t('onboarding.retry')}
              </Button>
            </footer>
          </main>
        </div>
      )
    }

    return (
      <div className='bg-background flex h-screen items-center justify-center'>
        <Loader2 className='text-muted-foreground size-5 animate-spin' />
      </div>
    )
  }

  return (
    <div className='bg-background flex h-screen flex-col overflow-hidden'>
      <header
        data-tauri-drag-region
        className='flex h-4 shrink-0 items-center justify-end px-2.5'
      >
        <div className='flex items-center gap-1'>
          {Array.from({ length: totalSteps }, (_, index) => (
            <div
              key={index}
              className={`h-1 rounded-full ${
                index === step
                  ? 'bg-foreground w-3'
                  : index < step
                    ? 'bg-foreground/45 w-2'
                    : 'bg-border w-2'
              }`}
            />
          ))}
        </div>
      </header>

      <main className='flex flex-1 flex-col px-2.5 pt-0.5 pb-1.5'>
        <div className='flex min-h-0 flex-1 items-center justify-center'>
          {step === 0 ? (
            <div className='flex w-full max-w-[180px] justify-center'>
              <CompactField label={t('config.language')} centered>
                <Select
                  value={draft.language}
                  onValueChange={(value) => {
                    handleConfigChange('language', value)
                  }}
                >
                  <SelectTrigger className='h-7 w-full rounded-md px-2 text-[11px]'>
                    <SelectValue
                      key={previewLanguage}
                      placeholder={t(`menu.languages.${draft.language}`)}
                    />
                  </SelectTrigger>
                  <SelectContent>
                    {locales.map((code) => (
                      <SelectItem
                        key={`${previewLanguage}:${code}`}
                        value={code}
                      >
                        {t(`menu.languages.${code}`)}
                      </SelectItem>
                    ))}
                  </SelectContent>
                </Select>
              </CompactField>
            </div>
          ) : null}

          {step === 1 ? (
            <div className='w-full max-w-[252px] space-y-1.5'>
              <Field
                label={t('config.runtime')}
                value={draft.runtimePath}
                onChange={(value) => handleConfigChange('runtimePath', value)}
              />
              <Field
                label={t('config.models')}
                value={draft.modelsPath}
                onChange={(value) => handleConfigChange('modelsPath', value)}
              />
            </div>
          ) : null}

          {step === 2 ? (
            <div className='w-full max-w-[252px] space-y-1.5'>
              <Field
                label={t('config.proxy')}
                value={draft.proxyUrl ?? ''}
                placeholder='http://127.0.0.1:7890'
                onChange={(value) =>
                  handleConfigChange('proxyUrl', value.trim() ? value : null)
                }
              />
              <div className='grid grid-cols-2 gap-1.5'>
                <MirrorField
                  label={t('config.pypi')}
                  value={draft.pypiMirror}
                  officialLabel={t('config.official')}
                  customLabel={t('config.custom')}
                  onChange={(value) => handleConfigChange('pypiMirror', value)}
                />
                <MirrorField
                  label={t('config.github')}
                  value={draft.githubMirror}
                  officialLabel={t('config.official')}
                  customLabel={t('config.custom')}
                  onChange={(value) =>
                    handleConfigChange('githubMirror', value)
                  }
                />
              </div>
            </div>
          ) : null}

          {step === 3 ? (
            <div className='w-full max-w-[220px]'>
              <div className='flex items-center gap-2 text-sm font-medium'>
                {initializeState === 'running' ? (
                  <Loader2 className='size-3.5 animate-spin' />
                ) : null}
                <span>
                  {initializeState === 'running'
                    ? t('onboarding.downloading')
                    : initializeState === 'done'
                      ? t('onboarding.opening')
                      : initializeState === 'failed'
                        ? t('onboarding.failed')
                        : t('common.initializing')}
                </span>
              </div>
              <div className='text-muted-foreground mt-1 truncate text-[11px]'>
                {currentDownload}
              </div>
              {initializeError ? (
                <div className='text-destructive mt-2 line-clamp-2 text-[11px]'>
                  {initializeError}
                </div>
              ) : null}
            </div>
          ) : null}
        </div>

        <footer className='border-border mt-1.5 flex h-8 shrink-0 items-center justify-between border-t pt-1.5'>
          <Button
            variant='ghost'
            size='xs'
            onClick={handleBack}
            disabled={step === 0 || saving}
          >
            {t('onboarding.back')}
          </Button>

          {step < totalSteps - 1 ? (
            <Button size='xs' onClick={handleNext} disabled={!canProceed}>
              {step === totalSteps - 2
                ? t('onboarding.start')
                : t('onboarding.next')}
            </Button>
          ) : (
            <Button
              size='xs'
              onClick={() => setInitializeRequestId((current) => current + 1)}
              disabled={saving || initializeState === 'done'}
            >
              {saving ? <Loader2 className='size-3 animate-spin' /> : null}
              {initializeState === 'failed'
                ? t('onboarding.retry')
                : t('onboarding.start')}
            </Button>
          )}
        </footer>
      </main>
    </div>
  )
}

function Field({
  label,
  value,
  onChange,
  placeholder,
}: {
  label: string
  value: string
  onChange: (value: string) => void
  placeholder?: string
}) {
  return (
    <div className='space-y-0.5'>
      <div className='text-muted-foreground text-[9px] font-medium tracking-wide uppercase'>
        {label}
      </div>
      <Input
        value={value}
        onChange={(event) => onChange(event.target.value)}
        placeholder={placeholder}
        spellCheck={false}
        className='h-7 rounded-md px-2 text-[11px]'
      />
    </div>
  )
}

function CompactField({
  label,
  children,
  centered = false,
}: {
  label: string
  children: ReactNode
  centered?: boolean
}) {
  return (
    <div className={`space-y-1 ${centered ? 'w-full text-center' : ''}`}>
      <div
        className={`text-muted-foreground text-[9px] font-medium tracking-wide uppercase ${centered ? 'text-center' : ''}`}
      >
        {label}
      </div>
      {children}
    </div>
  )
}

function MirrorField({
  label,
  value,
  officialLabel,
  customLabel,
  onChange,
}: {
  label: string
  value: Config['pypiMirror']
  officialLabel: string
  customLabel: string
  onChange: (value: Config['pypiMirror']) => void
}) {
  return (
    <div className='space-y-0.5'>
      <div className='text-muted-foreground text-[9px] font-medium tracking-wide uppercase'>
        {label}
      </div>
      <Select
        value={value.kind}
        onValueChange={(kind: 'official' | 'custom') => {
          onChange({
            kind,
            customBaseUrl: kind === 'custom' ? value.customBaseUrl : null,
          })
        }}
      >
        <SelectTrigger className='h-7 w-full rounded-md px-2 text-[11px]'>
          <SelectValue placeholder={officialLabel} />
        </SelectTrigger>
        <SelectContent>
          <SelectItem value='official'>{officialLabel}</SelectItem>
          <SelectItem value='custom'>{customLabel}</SelectItem>
        </SelectContent>
      </Select>
      {value.kind === 'custom' ? (
        <Input
          value={value.customBaseUrl ?? ''}
          onChange={(event) =>
            onChange({
              kind: 'custom',
              customBaseUrl: event.target.value,
            })
          }
          placeholder='https://mirror.example'
          spellCheck={false}
          className='h-7 rounded-md px-2 text-[11px]'
        />
      ) : null}
    </div>
  )
}
