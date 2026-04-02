'use client'

import {
  startTransition,
  useEffect,
  useCallback,
  useState,
  type ReactNode,
} from 'react'
import { useTranslation } from 'react-i18next'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select'
import { getConfig, updateConfig, initialize } from '@/lib/api/system/system'
import { listDownloads } from '@/lib/api/downloads/downloads'
import type { DownloadState } from '@/lib/api/schemas'
import type { BootstrapConfig } from '@/lib/api/schemas'
import i18n, { supportedLanguages } from '@/lib/i18n'

const DEFAULT_CONFIG: BootstrapConfig = {
  runtime: { path: '' },
  models: { path: '' },
  http: { proxy: null },
}

const STEP_COUNT = 4

type WizardStep = 0 | 1 | 2 | 3

type ActiveDownload = {
  filename: string
  percent?: number
  failed?: boolean
}

const computePercent = (download: DownloadState) =>
  download.total && download.total > 0
    ? Math.min(100, Math.round((download.downloaded / download.total) * 100))
    : undefined

const normalizeConfig = (next: BootstrapConfig): BootstrapConfig => ({
  runtime: { path: next.runtime.path.trim() },
  models: { path: next.models.path.trim() },
  http: {
    proxy: next.http.proxy?.trim() ? next.http.proxy.trim() : null,
  },
})

export default function BootstrapPage() {
  const { t } = useTranslation()

  const [step, setStep] = useState<WizardStep>(0)
  const [config, setConfig] = useState<BootstrapConfig>(DEFAULT_CONFIG)
  const [loading, setLoading] = useState(true)
  const [initializing, setInitializing] = useState(false)
  const [failed, setFailed] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const [download, setDownload] = useState<ActiveDownload | null>(null)

  const startInitialize = useCallback(async () => {
    const nextConfig = normalizeConfig(config)
    if (!nextConfig.runtime.path || !nextConfig.models.path) {
      setError(t('bootstrap.pathsRequired'))
      return
    }

    setConfig(nextConfig)
    setStep(3)
    setInitializing(true)
    setFailed(false)
    setError(null)

    try {
      const saved = await updateConfig(nextConfig)
      setConfig(saved)
      await initialize()
    } catch (cause) {
      setInitializing(false)
      setFailed(true)
      setError(cause instanceof Error ? cause.message : t('bootstrap.failed'))
    }
  }, [config, t])

  useEffect(() => {
    void (async () => {
      try {
        const saved = await getConfig()
        setConfig(saved)
      } catch (cause) {
        const message =
          cause instanceof Error
            ? cause.message
            : t('bootstrap.failedLoadConfig')
        setError(message)
      } finally {
        setLoading(false)
      }
    })()
  }, [])

  useEffect(() => {
    const interval = setInterval(async () => {
      try {
        const downloads = await listDownloads()
        const active =
          downloads.find(
            (entry) =>
              entry.status === 'started' || entry.status === 'downloading',
          ) ??
          downloads
            .slice()
            .sort((left, right) => left.filename.localeCompare(right.filename))
            .at(-1) ??
          null

        if (active) {
          setDownload({
            filename: active.filename,
            percent: computePercent(active),
            failed: active.status === 'failed',
          })
        }
      } catch {
        // Backend may not be ready yet during bootstrap
      }
    }, 1500)

    return () => clearInterval(interval)
  }, [])

  const goNext = async () => {
    setError(null)

    if (step === 0) {
      startTransition(() => setStep(1))
      return
    }

    if (step === 1) {
      const nextConfig = normalizeConfig(config)
      if (!nextConfig.runtime.path || !nextConfig.models.path) {
        setError(t('bootstrap.pathsRequired'))
        return
      }
      startTransition(() => setStep(2))
      return
    }

    if (step === 2) {
      await startInitialize()
    }
  }

  const goBack = () => {
    setInitializing(false)
    setFailed(false)
    setError(null)

    startTransition(() => {
      setStep((current) =>
        current > 0 ? ((current - 1) as WizardStep) : current,
      )
    })
  }

  const stepIndex = Math.min(step + 1, STEP_COUNT)
  const progressLabel = failed
    ? t('bootstrap.failed')
    : t('common.initializing')

  if (loading) {
    return (
      <main className='bg-background fixed inset-0 overflow-hidden'>
        <div className='flex h-full items-center justify-center'>
          <div className='text-muted-foreground text-xs uppercase'>
            {t('bootstrap.loading')}
          </div>
        </div>
      </main>
    )
  }

  return (
    <main className='from-background via-accent/20 to-background fixed inset-0 overflow-hidden bg-linear-to-br'>
      <StepIndicator current={stepIndex} total={STEP_COUNT} />

      <div className='grid h-full grid-rows-[1fr_34px] overflow-hidden'>
        <section className='overflow-hidden px-3 pt-4 pb-2'>
          {step === 0 && (
            <StepPane title={t('bootstrap.language')} error={error}>
              <div className='flex justify-center'>
                <Select
                  value={i18n.language}
                  onValueChange={(value) => void i18n.changeLanguage(value)}
                >
                  <SelectTrigger
                    size='sm'
                    className='bg-background/90 min-w-36 rounded-sm text-xs'
                  >
                    <SelectValue />
                  </SelectTrigger>
                  <SelectContent>
                    {supportedLanguages.map((language) => (
                      <SelectItem key={language} value={language}>
                        {t(`menu.languages.${language}`)}
                      </SelectItem>
                    ))}
                  </SelectContent>
                </Select>
              </div>
            </StepPane>
          )}

          {step === 1 && (
            <StepPane error={error}>
              <div className='grid gap-2'>
                <div className='grid gap-1'>
                  <Label
                    htmlFor='bootstrap-runtime-path'
                    className='text-[11px] font-medium'
                  >
                    {t('bootstrap.runtimePath')}
                  </Label>
                  <Input
                    id='bootstrap-runtime-path'
                    className='bg-background/90 h-8 rounded-sm px-2 text-xs shadow-none'
                    value={config.runtime.path}
                    onChange={(event) => {
                      setConfig((current) => ({
                        ...current,
                        runtime: { path: event.target.value },
                      }))
                    }}
                    placeholder={t('bootstrap.runtimePathPlaceholder')}
                  />
                </div>

                <div className='grid gap-1'>
                  <Label
                    htmlFor='bootstrap-models-path'
                    className='text-[11px] font-medium'
                  >
                    {t('bootstrap.modelsPath')}
                  </Label>
                  <Input
                    id='bootstrap-models-path'
                    className='bg-background/90 h-8 rounded-sm px-2 text-xs shadow-none'
                    value={config.models.path}
                    onChange={(event) => {
                      setConfig((current) => ({
                        ...current,
                        models: { path: event.target.value },
                      }))
                    }}
                    placeholder={t('bootstrap.modelsPathPlaceholder')}
                  />
                </div>
              </div>
            </StepPane>
          )}

          {step === 2 && (
            <StepPane error={error}>
              <div className='grid gap-1'>
                <Label
                  htmlFor='bootstrap-http-proxy'
                  className='text-[11px] font-medium'
                >
                  {t('bootstrap.proxyUrl')}
                </Label>
                <Input
                  id='bootstrap-http-proxy'
                  className='bg-background/90 h-8 rounded-sm px-2 text-xs shadow-none'
                  value={config.http.proxy ?? ''}
                  onChange={(event) => {
                    setConfig((current) => ({
                      ...current,
                      http: { proxy: event.target.value || null },
                    }))
                  }}
                  placeholder={t('bootstrap.proxyUrlPlaceholder')}
                />
              </div>
            </StepPane>
          )}

          {step === 3 && (
            <StepPane title={progressLabel} error={error}>
              <div className='grid gap-2'>
                <div className='bg-muted relative h-1.5 overflow-hidden rounded-full'>
                  {download && typeof download.percent === 'number' ? (
                    <div
                      className={`h-full transition-[width] duration-300 ${
                        download.failed ? 'bg-destructive' : 'bg-primary'
                      }`}
                      style={{ width: `${download.percent}%` }}
                    />
                  ) : (
                    <div className='activity-progress-indeterminate from-primary/40 via-primary to-primary/40 absolute inset-y-0 left-0 w-1/2 rounded-full bg-linear-to-r' />
                  )}
                </div>

                <div className='text-muted-foreground truncate text-center text-[10px] leading-tight'>
                  {download?.filename ?? t('bootstrap.waitingForDownload')}
                  {download && typeof download.percent === 'number'
                    ? ` ${download.percent}%`
                    : ''}
                </div>
              </div>
            </StepPane>
          )}
        </section>

        <div className='border-border/70 flex overflow-hidden border-t'>
          {step < 3 ? (
            <div className='grid h-full w-full grid-cols-2'>
              <Button
                variant='ghost'
                size='xs'
                className='border-border/70 h-full rounded-none border-r px-2 text-[11px] uppercase'
                disabled={step === 0 || initializing}
                onClick={goBack}
              >
                {t('bootstrap.back')}
              </Button>
              <Button
                size='xs'
                className='h-full rounded-none px-2 text-[11px] uppercase'
                onClick={() => {
                  void goNext()
                }}
              >
                {step === 2 ? t('bootstrap.init') : t('bootstrap.next')}
              </Button>
            </div>
          ) : (
            <Button
              size='xs'
              className='h-full rounded-none px-2 text-[11px] uppercase'
              disabled={initializing}
              onClick={() => {
                void startInitialize()
              }}
            >
              {t('bootstrap.retryNow')}
            </Button>
          )}
        </div>
      </div>
    </main>
  )
}

function StepPane({
  title,
  children,
  error,
}: {
  title?: string
  children: ReactNode
  error: string | null
}) {
  return (
    <div className='flex h-full flex-col justify-center gap-3'>
      {title ? (
        <div className='text-center'>
          <div className='text-muted-foreground text-[10px] font-semibold tracking-[0.18em] uppercase'>
            {title}
          </div>
        </div>
      ) : null}
      {children}
      {error && (
        <div
          title={error}
          className='text-destructive [display:-webkit-box] max-h-[2.8rem] overflow-hidden text-center text-[9px] leading-[1.15] break-words [-webkit-box-orient:vertical] [-webkit-line-clamp:4]'
        >
          {error}
        </div>
      )}
    </div>
  )
}

function StepIndicator({ current, total }: { current: number; total: number }) {
  return (
    <div className='absolute top-2 right-2 z-10 flex items-center gap-1'>
      {Array.from({ length: total }).map((_, index) => (
        <span
          key={index}
          className={`block h-1 w-4 rounded-full transition-colors ${
            index < current ? 'bg-primary' : 'bg-border'
          }`}
        />
      ))}
    </div>
  )
}
