'use client'

import { Languages, LoaderCircle, Scan, ScanText, WandSparkles, X } from 'lucide-react'
import { useEffect, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'

import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { Popover, PopoverContent, PopoverTrigger } from '@/components/ui/popover'
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select'
import { Separator } from '@/components/ui/separator'
import { Textarea } from '@/components/ui/textarea'
import {
  defaultTranslationProvider,
  koharuClient,
  normalizeTargetLanguage,
  translationProviderLabels,
  translationProviders,
  useEditorStore,
  type Providers,
  type Scope,
  type Stage,
  type TargetLanguageView,
  type TranslationCredentialsView,
  type TranslationSettings,
} from '@/lib/koharu'

const stages = [
  { id: 'detection', icon: Scan },
  { id: 'ocr', icon: ScanText },
  { id: 'translation', icon: Languages },
  { id: 'inpainting', icon: WandSparkles },
] as const satisfies ReadonlyArray<{ id: Stage; icon: typeof Scan }>

const noTargetLanguages: TargetLanguageView[] = []

export function CanvasToolbar() {
  return (
    <div className='flex min-h-11 shrink-0 items-center gap-2 border-b border-border/60 bg-card px-3 py-2 text-xs text-foreground'>
      <WorkflowButtons />
      <div className='flex-1' />
      <TranslationQuickSettings />
    </div>
  )
}

function WorkflowButtons() {
  const { t } = useTranslation()
  const page = useEditorStore((state) => state.page)
  const selectedElements = useEditorStore((state) => state.selectedElements)
  const selectedPages = useEditorStore((state) => state.selectedPages)
  const jobs = useEditorStore((state) => state.jobs)
  const pipelineJob = Object.values(jobs).find(
    (job) => job.state === 'running' && job.kind === 'pipeline',
  )

  const run = (stage: Stage) => {
    if (!page) return
    koharuClient.fire({
      type: 'run_pipeline',
      scope: pipelineScope(stage, page.id, selectedPages, selectedElements),
      target: { target: 'exact', stages: [stage] },
    })
  }

  return (
    <div className='flex items-center gap-0.5'>
      {stages.map(({ id, icon: Icon }, index) => {
        const label = t(`native.phase.${id}`, { defaultValue: id })
        const running = pipelineJob?.state === 'running' && pipelineJob.phase === id
        return (
          <div key={id} className='contents'>
            {index > 0 && <Separator orientation='vertical' className='mx-0.5 h-4' />}
            <Button
              variant='ghost'
              size='xs'
              title={`${t('native.canvas.run', { defaultValue: 'Run' })} ${label}`}
              data-testid={`toolbar-${id}`}
              disabled={!page || Boolean(pipelineJob)}
              onClick={() => run(id)}
            >
              {running ? (
                <LoaderCircle className='size-4 animate-spin' />
              ) : (
                <Icon className='size-4' />
              )}
              {label}
            </Button>
          </div>
        )
      })}
    </div>
  )
}

function TranslationQuickSettings() {
  const { t, i18n } = useTranslation()
  const settings = useEditorStore((state) => state.settings)
  const jobs = useEditorStore((state) => state.jobs)
  const [open, setOpen] = useState(false)
  const [saving, setSaving] = useState(false)
  const [draft, setDraft] = useState<TranslationSettings | null>(settings?.translation ?? null)
  const draftRef = useRef(draft)
  const saveTimer = useRef<number | null>(null)
  const saveVersion = useRef(0)
  const languages = settings?.target_languages ?? noTargetLanguages
  const translating = Object.values(jobs).some(
    (job) => job.state === 'running' && job.kind === 'pipeline' && job.phase === 'translation',
  )
  const languageNames = new Intl.DisplayNames([i18n.resolvedLanguage ?? i18n.language], {
    type: 'language',
  })

  useEffect(
    () => () => {
      if (saveTimer.current !== null) window.clearTimeout(saveTimer.current)
    },
    [],
  )

  useEffect(() => {
    if (!open && saveTimer.current === null && !saving) {
      draftRef.current = settings?.translation ?? null
      setDraft(settings?.translation ?? null)
    }
  }, [open, saving, settings])

  const persist = async (translation: TranslationSettings, version: number) => {
    const current = useEditorStore.getState().settings
    if (!current) return
    setSaving(true)
    try {
      await koharuClient.command({
        type: 'set_settings',
        pipeline: current.pipeline,
        translation,
      })
    } catch {
      // The bridge reports command failures through the shared editor error state.
    } finally {
      if (version === saveVersion.current) {
        setSaving(false)
        const synchronized = useEditorStore.getState().settings?.translation
        if (synchronized) {
          draftRef.current = synchronized
          setDraft(synchronized)
        }
      }
    }
  }

  const update = (translation: TranslationSettings, delay = 350) => {
    draftRef.current = translation
    setDraft(translation)
    const version = ++saveVersion.current
    if (saveTimer.current !== null) window.clearTimeout(saveTimer.current)
    saveTimer.current = window.setTimeout(() => {
      saveTimer.current = null
      void persist(translation, version)
    }, delay)
  }

  const changeOpen = (next: boolean) => {
    if (next) {
      const translation = settings?.translation ?? null
      draftRef.current = translation
      setDraft(translation)
    } else if (saveTimer.current !== null && draftRef.current) {
      window.clearTimeout(saveTimer.current)
      saveTimer.current = null
      void persist(draftRef.current, saveVersion.current)
    }
    setOpen(next)
  }

  const triggerLabel = settings
    ? translationModelName(settings.translation.model)
    : t('native.settings.translation', { defaultValue: 'Translation' })

  return (
    <Popover open={open} onOpenChange={changeOpen}>
      <PopoverTrigger asChild>
        <button
          data-testid='llm-trigger'
          disabled={!settings}
          title={triggerLabel}
          className='flex h-6 max-w-44 cursor-pointer items-center gap-1.5 rounded-full bg-primary px-2.5 text-[11px] font-medium text-primary-foreground shadow-sm ring-1 ring-primary/30 transition hover:bg-primary/90 disabled:cursor-default disabled:opacity-50'
        >
          <span
            className={`size-1.5 rounded-full bg-white ${translating ? 'animate-pulse' : 'opacity-80'}`}
          />
          <span className='truncate'>{triggerLabel}</span>
        </button>
      </PopoverTrigger>
      <PopoverContent align='end' className='w-[288px] p-0' data-testid='llm-popover'>
        <div className='flex items-center justify-between border-b border-border px-2.5 py-2'>
          <p className='text-xs font-semibold'>
            {t('native.settings.translation', { defaultValue: 'Translation' })}
          </p>
          <span className='flex h-4 items-center gap-1 text-[10px] text-muted-foreground'>
            {saving && <LoaderCircle className='size-3 animate-spin' />}
            {saving ? t('common.saving', { defaultValue: 'Saving…' }) : ''}
          </span>
        </div>
        {draft && settings && (
          <div className='grid gap-2.5 p-2.5'>
            <div className={`grid gap-2 ${'model' in draft.model ? 'grid-cols-2' : 'grid-cols-1'}`}>
              <div className='grid gap-1'>
                <Label htmlFor='quick-translation-provider' className='text-[10px] leading-none'>
                  {t('native.settings.provider', { defaultValue: 'Provider' })}
                </Label>
                <Select
                  value={draft.model.provider}
                  onValueChange={(provider) =>
                    update(
                      {
                        ...draft,
                        model: defaultTranslationProvider(provider as Providers['provider']),
                      },
                      0,
                    )
                  }
                >
                  <SelectTrigger id='quick-translation-provider' className='h-7 w-full text-xs'>
                    <SelectValue />
                  </SelectTrigger>
                  <SelectContent>
                    {translationProviders.map((provider) => (
                      <SelectItem key={provider} value={provider}>
                        {translationProviderLabels[provider]}
                      </SelectItem>
                    ))}
                  </SelectContent>
                </Select>
              </div>
              {'model' in draft.model && (
                <QuickModelField
                  model={draft.model}
                  localModels={settings.local_translation_models}
                  onChange={(model) => update({ ...draft, model })}
                />
              )}
            </div>
            {credentialField(draft.model.provider) && (
              <QuickCredentialField
                provider={credentialField(draft.model.provider)!}
                credentials={draft.credentials}
                onChange={(credentials, delay) => update({ ...draft, credentials }, delay)}
              />
            )}
            <div className='grid gap-1'>
              <Label htmlFor='quick-target-language' className='text-[10px] leading-none'>
                {t('native.model.targetLanguage', { defaultValue: 'Target language' })}
              </Label>
              <Select
                value={normalizeTargetLanguage(draft.target_language, languages)}
                disabled={languages.length === 0}
                onValueChange={(target_language) => update({ ...draft, target_language }, 0)}
              >
                <SelectTrigger id='quick-target-language' className='h-7 w-full text-xs'>
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  {languages.map((language) => (
                    <SelectItem key={language.tag} value={language.tag}>
                      {languageNames.of(language.tag) ?? language.name}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
            </div>
            <div className='grid gap-1'>
              <Label htmlFor='quick-translation-instructions' className='text-[10px] leading-none'>
                {t('native.model.instructions', { defaultValue: 'Instructions' })}
              </Label>
              <Textarea
                id='quick-translation-instructions'
                className='min-h-14 resize-y text-xs'
                value={draft.instructions ?? ''}
                placeholder={t('native.model.instructionsPlaceholder', {
                  defaultValue: 'Optional guidance for tone, names, and formatting.',
                })}
                onChange={(event) =>
                  update({ ...draft, instructions: event.currentTarget.value || null }, 500)
                }
              />
            </div>
          </div>
        )}
      </PopoverContent>
    </Popover>
  )
}

function QuickModelField({
  model,
  localModels,
  onChange,
}: {
  model: Providers
  localModels: string[]
  onChange: (model: Providers) => void
}) {
  const { t } = useTranslation()
  if (!('model' in model)) return <div />
  const label =
    model.provider === 'local'
      ? t('native.model.localModel', { defaultValue: 'Local model' })
      : t('native.model.remoteModel', { defaultValue: 'Remote model' })

  return (
    <div className='grid gap-1'>
      <Label htmlFor='quick-translation-model' className='text-[10px] leading-none'>
        {label}
      </Label>
      {model.provider === 'local' ? (
        <Select value={model.model} onValueChange={(value) => onChange({ ...model, model: value })}>
          <SelectTrigger id='quick-translation-model' className='h-7 w-full text-xs'>
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            {[...new Set([model.model, ...localModels])].map((localModel) => (
              <SelectItem key={localModel} value={localModel}>
                {localModel}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
      ) : (
        <Input
          id='quick-translation-model'
          className='h-7 text-xs'
          value={model.model}
          onChange={(event) => onChange({ ...model, model: event.currentTarget.value })}
        />
      )}
    </div>
  )
}

function QuickCredentialField({
  provider,
  credentials,
  onChange,
}: {
  provider: keyof TranslationCredentialsView
  credentials: TranslationCredentialsView
  onChange: (credentials: TranslationCredentialsView, delay: number) => void
}) {
  const { t } = useTranslation()
  const credential = credentials[provider]
  const configured = !credential.clear && (credential.configured || Boolean(credential.value))

  return (
    <div className='grid gap-1'>
      <div className='flex items-center justify-between'>
        <Label htmlFor='quick-translation-credential' className='text-[10px] leading-none'>
          {t('native.settings.credentials', { defaultValue: 'Credential' })}
        </Label>
        {configured && (
          <span className='text-[9px] text-muted-foreground'>
            {t('native.settings.configured', { defaultValue: 'Configured' })}
          </span>
        )}
      </div>
      <div className='flex gap-1'>
        <Input
          id='quick-translation-credential'
          aria-label={`${provider.replaceAll('_', ' ')} credential`}
          type='password'
          autoComplete='new-password'
          className='h-7 min-w-0 flex-1 text-xs [&::-ms-reveal]:hidden'
          value={credential.value ?? ''}
          placeholder={
            configured
              ? t('native.settings.configured', { defaultValue: 'Configured' })
              : t('native.settings.notConfigured', { defaultValue: 'Not configured' })
          }
          onChange={(event) =>
            onChange(
              {
                ...credentials,
                [provider]: {
                  configured: credential.configured,
                  value: event.currentTarget.value || null,
                  clear: false,
                },
              },
              650,
            )
          }
        />
        {configured && (
          <Button
            size='icon-xs'
            variant='ghost'
            title={t('native.settings.clear', { defaultValue: 'Clear' })}
            aria-label={t('native.settings.clear', { defaultValue: 'Clear' })}
            onClick={() =>
              onChange(
                {
                  ...credentials,
                  [provider]: { configured: false, value: null, clear: true },
                },
                0,
              )
            }
          >
            <X />
          </Button>
        )}
      </div>
    </div>
  )
}

function credentialField(provider: Providers['provider']): keyof TranslationCredentialsView | null {
  return provider === 'local' ? null : provider
}

function translationModelName(model: Providers): string {
  return 'model' in model && model.model.trim()
    ? model.model
    : translationProviderLabels[model.provider]
}

function pipelineScope(
  stage: Stage,
  page: string,
  selectedPages: string[],
  selectedElements: string[],
): Scope {
  if (selectedElements.length > 0 && (stage === 'ocr' || stage === 'translation')) {
    return { scope: 'entities', value: selectedElements }
  }
  return { scope: 'pages', value: selectedPages.length > 0 ? selectedPages : [page] }
}
