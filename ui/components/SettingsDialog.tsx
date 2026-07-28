'use client'

import {
  Cpu,
  Eraser,
  Eye,
  EyeOff,
  FileText,
  Keyboard,
  Languages,
  Monitor,
  Moon,
  Palette,
  Save,
  Search,
  Sparkles,
  Sun,
} from 'lucide-react'
import { useTheme } from 'next-themes'
import { useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'

import {
  Accordion,
  AccordionContent,
  AccordionItem,
  AccordionTrigger,
} from '@/components/ui/accordion'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Dialog, DialogContent, DialogDescription, DialogTitle } from '@/components/ui/dialog'
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
import { Switch } from '@/components/ui/switch'
import { Textarea } from '@/components/ui/textarea'
import { supportedLanguages } from '@/lib/i18n'
import {
  koharuClient,
  defaultTranslationProvider,
  normalizeTargetLanguage,
  translationProviderLabels,
  translationProviders,
  useEditorStore,
  type DetectionModel,
  type InpaintingModel,
  type LaMaHDStrategy,
  type OcrModel,
  type PipelineConfig,
  type ShortcutAction,
  type Stage,
  type TargetLanguageView,
  type TranslationCredentialsView,
  type TranslationSettings,
  type Providers,
} from '@/lib/koharu'

const settingsTabs = [
  { id: 'appearance', icon: Palette, label: 'native.settings.appearance' },
  { id: 'pipeline', icon: Cpu, label: 'native.settings.pipeline' },
  { id: 'translation', icon: Languages, label: 'native.settings.translation' },
  { id: 'shortcuts', icon: Keyboard, label: 'native.settings.shortcuts' },
] as const
type SettingsTab = (typeof settingsTabs)[number]['id']

type PipelineModel = DetectionModel | OcrModel | InpaintingModel
type ModelPhase = Exclude<Stage, 'translation'>
type ModelName = PipelineModel['model'] | Providers['provider']
const modelOptions = {
  detection: ['koharu-layout-rfdetr-seg-2xl'],
  ocr: ['paddleocr-vl-1.6', 'manga-ocr', 'baberu-ocr'],
  inpainting: ['lama', 'aot-inpainting', 'flux2-klein', 'rorem-mixed'],
} satisfies Record<ModelPhase, PipelineModel['model'][]>
const modelLabels: Record<ModelName, string> = {
  'koharu-layout-rfdetr-seg-2xl': 'Koharu Layout RF-DETR Seg 2XL',
  'paddleocr-vl-1.6': 'PaddleOCR-VL 1.6',
  'manga-ocr': 'Manga OCR',
  'baberu-ocr': 'Baberu OCR',
  ...translationProviderLabels,
  lama: 'LaMa',
  'aot-inpainting': 'AOT Inpainting',
  'flux2-klein': 'FLUX.2 Klein',
  'rorem-mixed': 'RORem Mixed',
}
const phaseDescriptions: Record<Stage, string> = {
  detection: 'Locate page regions and produce cleanup masks.',
  ocr: 'Read the text inside each region.',
  translation: 'Convert source text to the target language.',
  inpainting: 'Rebuild the artwork behind removed text.',
}

const phaseIcons = {
  detection: Search,
  ocr: FileText,
  inpainting: Eraser,
} satisfies Record<ModelPhase, typeof Search>

const pipelineGroups = [
  {
    id: 'analysis',
    phases: ['detection', 'ocr'],
    title: 'Page analysis',
    description: 'Find regions, infer text appearance, and read the source text.',
  },
  {
    id: 'restoration',
    phases: ['inpainting'],
    title: 'Artwork restoration',
    description: 'Reconstruct the artwork beneath removed source text.',
  },
] as const satisfies ReadonlyArray<{
  id: string
  phases: readonly ModelPhase[]
  title: string
  description: string
}>

export function SettingsDialog() {
  const { t } = useTranslation()
  const { theme, setTheme } = useTheme()
  const open = useEditorStore((state) => state.settingsOpen)
  const setOpen = useEditorStore((state) => state.setSettingsOpen)
  const settings = useEditorStore((state) => state.settings)
  const [draft, setDraft] = useState<PipelineConfig | null>(settings?.pipeline ?? null)
  const [translationDraft, setTranslationDraft] = useState<TranslationSettings | null>(
    settings?.translation ?? null,
  )
  const [tab, setTab] = useState<SettingsTab>('appearance')

  useEffect(() => {
    if (open) {
      setTab('appearance')
      koharuClient.fire({ type: 'get_settings' })
    }
  }, [open])
  useEffect(() => {
    setDraft(settings?.pipeline ?? null)
    setTranslationDraft(settings?.translation ?? null)
  }, [open, settings])

  const save = () => {
    if (!draft || !translationDraft) return
    koharuClient
      .command({ type: 'set_settings', pipeline: draft, translation: translationDraft })
      .then((result) => {
        if (result === 'accepted') {
          setOpen(false)
        }
      })
      .catch(() => undefined)
  }

  return (
    <Dialog open={open} onOpenChange={setOpen}>
      <DialogContent className='flex h-[720px] max-h-[92vh] w-[880px] max-w-[94vw] flex-col gap-0 overflow-hidden p-0'>
        <DialogTitle className='sr-only'>
          {t('native.settings.title', { defaultValue: 'Settings' })}
        </DialogTitle>
        <DialogDescription className='sr-only'>
          {t('native.settings.description', { defaultValue: 'Koharu settings' })}
        </DialogDescription>

        <div className='flex min-h-0 flex-1'>
          <nav className='flex w-[188px] shrink-0 flex-col gap-1 border-r border-border bg-muted/30 p-3'>
            <p className='mb-3 px-3 text-[10px] font-semibold tracking-widest text-muted-foreground uppercase'>
              {t('native.settings.title', { defaultValue: 'Settings' })}
            </p>
            {settingsTabs.map(({ id, icon: Icon, label }) => (
              <button
                key={id}
                type='button'
                data-active={tab === id}
                onClick={() => setTab(id)}
                className='flex items-center gap-3 rounded-lg px-3 py-2 text-left text-sm text-muted-foreground transition hover:text-foreground data-[active=true]:bg-accent data-[active=true]:text-accent-foreground'
              >
                <Icon className='size-4 shrink-0' />
                {t(label, { defaultValue: id })}
              </button>
            ))}
          </nav>

          <div className='flex min-w-0 flex-1 flex-col'>
            <ScrollArea className='min-h-0 flex-1'>
              <div className='p-6 lg:p-7'>
                {tab === 'appearance' && (
                  <AppearanceSettings theme={theme ?? 'system'} onThemeChange={setTheme} />
                )}

                {tab === 'pipeline' && (
                  <>
                    {draft ? (
                      <PipelineSettings config={draft} onChange={setDraft} />
                    ) : (
                      <UnavailableSettings />
                    )}
                  </>
                )}

                {tab === 'translation' && (
                  <>
                    {translationDraft ? (
                      <TranslationEditor
                        config={translationDraft}
                        localModels={settings?.local_translation_models ?? []}
                        targetLanguages={settings?.target_languages ?? []}
                        onChange={setTranslationDraft}
                      />
                    ) : (
                      <UnavailableSettings />
                    )}
                  </>
                )}

                {tab === 'shortcuts' && <ShortcutSettings />}
              </div>
            </ScrollArea>

            <div className='flex justify-end gap-2 border-t px-5 py-3'>
              <Button variant='outline' onClick={() => setOpen(false)}>
                {t('common.cancel', { defaultValue: 'Cancel' })}
              </Button>
              <Button disabled={!draft || !translationDraft} onClick={save}>
                <Save />
                {t('common.save', { defaultValue: 'Save' })}
              </Button>
            </div>
          </div>
        </div>
      </DialogContent>
    </Dialog>
  )
}

const themes = [
  { value: 'light', icon: Sun, label: 'native.settings.light' },
  { value: 'dark', icon: Moon, label: 'native.settings.dark' },
  { value: 'system', icon: Monitor, label: 'native.settings.system' },
] as const

function AppearanceSettings({
  theme,
  onThemeChange,
}: {
  theme: string
  onThemeChange: (theme: string) => void
}) {
  const { t, i18n } = useTranslation()
  return (
    <div className='space-y-8'>
      <Section title={t('native.settings.theme', { defaultValue: 'Theme' })}>
        <div className='grid grid-cols-3 gap-3'>
          {themes.map(({ value, icon: Icon, label }) => (
            <button
              key={value}
              type='button'
              data-active={theme === value}
              onClick={() => onThemeChange(value)}
              className='flex flex-col items-center gap-2 rounded-xl border border-border bg-card px-4 py-4 text-muted-foreground transition hover:border-foreground/30 data-[active=true]:border-primary data-[active=true]:text-foreground'
            >
              <Icon className='size-5' />
              <span className='text-xs font-medium'>{t(label, { defaultValue: value })}</span>
            </button>
          ))}
        </div>
      </Section>

      <Section title={t('native.settings.language', { defaultValue: 'Language' })}>
        <Select value={i18n.language} onValueChange={(language) => i18n.changeLanguage(language)}>
          <SelectTrigger className='w-full'>
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
      </Section>
    </div>
  )
}

function Section({
  title,
  description,
  children,
}: {
  title: string
  description?: string
  children: React.ReactNode
}) {
  return (
    <section className='space-y-3'>
      <div>
        <h3 className='text-sm font-semibold text-foreground'>{title}</h3>
        {description && (
          <p className='mt-0.5 text-xs leading-relaxed text-muted-foreground'>{description}</p>
        )}
      </div>
      {children}
    </section>
  )
}

function SettingsPage({
  title,
  description,
  children,
}: {
  title: string
  description: string
  children: React.ReactNode
}) {
  return (
    <section className='space-y-7'>
      <header>
        <h2 className='text-xl font-semibold tracking-tight text-foreground'>{title}</h2>
        <p className='mt-1 max-w-2xl text-sm leading-relaxed text-muted-foreground'>
          {description}
        </p>
      </header>
      {children}
    </section>
  )
}

function SettingsGroup({
  title,
  description,
  children,
}: {
  title: string
  description?: string
  children: React.ReactNode
}) {
  return (
    <section className='space-y-2.5'>
      <div className='px-0.5'>
        <h3 className='text-sm font-semibold text-foreground'>{title}</h3>
        {description && (
          <p className='mt-0.5 text-xs leading-relaxed text-muted-foreground'>{description}</p>
        )}
      </div>
      <div className='divide-y divide-border overflow-hidden rounded-xl border border-border bg-card shadow-xs'>
        {children}
      </div>
    </section>
  )
}

function UnavailableSettings() {
  const { t } = useTranslation()
  return (
    <div className='rounded-xl border border-dashed bg-muted/20 p-6 text-sm text-muted-foreground'>
      {t('native.settings.unavailable', {
        defaultValue: 'Settings are unavailable while disconnected.',
      })}
    </div>
  )
}

function PipelineSettings({
  config,
  onChange,
}: {
  config: PipelineConfig
  onChange: (config: PipelineConfig) => void
}) {
  const { t } = useTranslation()
  return (
    <SettingsPage
      title={t('native.settings.pipeline', { defaultValue: 'Pipeline' })}
      description={t('native.settings.pipelineHelp', {
        defaultValue: 'Choose the models Koharu uses to analyze and restore each page.',
      })}
    >
      <div className='flex items-start gap-3 rounded-xl border border-primary/15 bg-primary/[0.045] p-4'>
        <div className='flex size-8 shrink-0 items-center justify-center rounded-lg bg-primary/10 text-primary'>
          <Sparkles className='size-4' />
        </div>
        <div className='min-w-0 flex-1'>
          <div className='flex flex-wrap items-center gap-2'>
            <p className='text-sm font-medium text-foreground'>
              {t('native.settings.pipelineManaged', { defaultValue: 'Managed automatically' })}
            </p>
            <Badge variant='outline' className='border-primary/20 bg-background/70 text-[10px]'>
              {t('native.settings.bestEffort', { defaultValue: 'Best effort' })}
            </Badge>
          </div>
          <p className='mt-1 text-xs leading-relaxed text-muted-foreground'>
            {t('native.settings.pipelineManagedHelp', {
              defaultValue:
                'Models download when needed, independent work may run together, and Koharu manages memory pressure for you.',
            })}
          </p>
        </div>
      </div>

      {pipelineGroups.map((group) => (
        <SettingsGroup
          key={group.id}
          title={t(`native.settings.pipelineGroup.${group.id}`, { defaultValue: group.title })}
          description={t(`native.settings.pipelineGroupHelp.${group.id}`, {
            defaultValue: group.description,
          })}
        >
          {group.phases.map((phase) => (
            <PhaseEditor key={phase} phase={phase} config={config} onChange={onChange} />
          ))}
        </SettingsGroup>
      ))}
    </SettingsPage>
  )
}

function PhaseEditor({
  phase,
  config,
  onChange,
}: {
  phase: ModelPhase
  config: PipelineConfig
  onChange: (config: PipelineConfig) => void
}) {
  const { t } = useTranslation()
  const current = config[phase] ?? defaultPipelineModel(modelOptions[phase][0]!)
  const PhaseIcon = phaseIcons[phase]
  const phaseName = t(`native.phase.${phase}`, { defaultValue: phase })
  return (
    <article className='grid min-w-0 gap-4 p-4 sm:grid-cols-[minmax(0,0.85fr)_minmax(260px,1.15fr)] sm:gap-6'>
      <div className='flex min-w-0 items-start gap-3'>
        <div className='flex size-8 shrink-0 items-center justify-center rounded-lg bg-muted text-muted-foreground'>
          <PhaseIcon className='size-4' />
        </div>
        <div className='min-w-0 pt-0.5'>
          <h4 className='text-sm font-medium text-foreground'>{phaseName}</h4>
          <p
            id={`pipeline-${phase}-description`}
            className='mt-1 text-xs leading-relaxed text-muted-foreground'
          >
            {t(`native.phaseDescription.${phase}`, { defaultValue: phaseDescriptions[phase] })}
          </p>
        </div>
      </div>
      <div
        className='grid min-w-0 content-start gap-1'
        aria-describedby={`pipeline-${phase}-description`}
      >
        <Label htmlFor={`pipeline-${phase}-model`} className='sr-only'>
          {t('native.settings.processor', { defaultValue: `${phaseName} model` })}
        </Label>
        <Select
          value={current.model}
          onValueChange={(model) =>
            onChange(
              setPhaseModel(config, phase, defaultPipelineModel(model as PipelineModel['model'])),
            )
          }
        >
          <SelectTrigger
            id={`pipeline-${phase}-model`}
            aria-label={`${phaseName} model`}
            className='w-full bg-background'
          >
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            {modelOptions[phase].map((model) => (
              <SelectItem key={model} value={model}>
                {modelLabels[model]}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
        {hasPipelineModelOptions(current) && (
          <Accordion type='single' collapsible>
            <AccordionItem value='options' className='border-0'>
              <AccordionTrigger className='py-2 text-xs font-medium text-muted-foreground hover:text-foreground hover:no-underline'>
                {t('native.settings.modelOptions', { defaultValue: 'Model options' })}
              </AccordionTrigger>
              <AccordionContent className='pt-1 pb-1'>
                <PipelineModelFields
                  model={current}
                  onChange={(model) => onChange(setPhaseModel(config, phase, model))}
                />
              </AccordionContent>
            </AccordionItem>
          </Accordion>
        )}
      </div>
    </article>
  )
}

function TranslationEditor({
  config,
  localModels,
  targetLanguages,
  onChange,
}: {
  config: TranslationSettings
  localModels: string[]
  targetLanguages: TargetLanguageView[]
  onChange: (config: TranslationSettings) => void
}) {
  const { t } = useTranslation()
  const replace = (model: Providers) => onChange({ ...config, model })
  const credential = credentialField(config.model.provider)
  const edit = credential ? config.credentials[credential] : null
  const value = edit?.value ?? ''
  const configured = edit ? !edit.clear && (edit.configured || value.length > 0) : false
  const [revealCredential, setRevealCredential] = useState(false)

  useEffect(() => setRevealCredential(false), [credential])

  return (
    <SettingsPage
      title={t('native.settings.translation', { defaultValue: 'Translation' })}
      description={t('native.settings.translationHelp', {
        defaultValue:
          'Choose the translation engine and the defaults used for every newly processed page.',
      })}
    >
      <SettingsGroup
        title={t('native.settings.translationEngine', { defaultValue: 'Engine' })}
        description={t('native.settings.translationEngineHelp', {
          defaultValue: 'Select a local model or connect a translation provider.',
        })}
      >
        <div className='space-y-4 p-4'>
          <div className='grid gap-1.5'>
            <Label htmlFor='translation-provider' className='text-xs font-medium'>
              {t('native.settings.provider', { defaultValue: 'Provider' })}
            </Label>
            <Select
              value={config.model.provider}
              onValueChange={(provider) =>
                replace(defaultTranslationProvider(provider as Providers['provider']))
              }
            >
              <SelectTrigger id='translation-provider' className='w-full bg-background'>
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                {translationProviders.map((option) => (
                  <SelectItem key={option} value={option}>
                    {modelLabels[option]}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
          </div>
          {hasProviderOptions(config.model) && (
            <div className='border-t border-border/70 pt-4'>
              <ProviderFields model={config.model} localModels={localModels} onChange={replace} />
            </div>
          )}
        </div>
        {credential && (
          <div className='grid gap-1.5 p-4'>
            <div className='flex items-center justify-between gap-3'>
              <Label
                htmlFor={`translation-credential-${credential}`}
                className='text-xs font-medium'
              >
                {t('native.settings.credentials', { defaultValue: 'Credential' })}
              </Label>
              {configured && (
                <Badge variant='secondary' className='text-[10px]'>
                  {t('native.settings.configured', { defaultValue: 'Configured' })}
                </Badge>
              )}
            </div>
            <div className='flex gap-2'>
              <Input
                id={`translation-credential-${credential}`}
                aria-label={`${credential.replaceAll('_', ' ')} credential`}
                type={revealCredential ? 'text' : 'password'}
                autoComplete='new-password'
                className='[&::-ms-reveal]:hidden'
                value={value}
                placeholder={
                  configured
                    ? t('native.settings.configured', { defaultValue: 'Configured' })
                    : t('native.settings.notConfigured', { defaultValue: 'Not configured' })
                }
                onChange={(event) =>
                  onChange({
                    ...config,
                    credentials: {
                      ...config.credentials,
                      [credential]: {
                        configured: config.credentials[credential].configured,
                        value: event.currentTarget.value || null,
                        clear: false,
                      },
                    },
                  })
                }
              />
              <Button
                type='button'
                size='icon-sm'
                variant='outline'
                disabled={!value}
                aria-label={
                  revealCredential
                    ? t('native.settings.hideCredential', { defaultValue: 'Hide credential' })
                    : t('native.settings.revealCredential', { defaultValue: 'Reveal credential' })
                }
                onClick={() => setRevealCredential((reveal) => !reveal)}
              >
                {revealCredential ? <EyeOff /> : <Eye />}
              </Button>
              {configured && (
                <Button
                  size='sm'
                  variant='destructive'
                  onClick={() =>
                    onChange({
                      ...config,
                      credentials: {
                        ...config.credentials,
                        [credential]: { configured: false, value: null, clear: true },
                      },
                    })
                  }
                >
                  {t('native.settings.clear', { defaultValue: 'Clear' })}
                </Button>
              )}
            </div>
          </div>
        )}
      </SettingsGroup>

      <SettingsGroup
        title={t('native.settings.translationOutput', { defaultValue: 'Output' })}
        description={t('native.settings.translationOutputHelp', {
          defaultValue: 'Set the destination language and reusable guidance for translated text.',
        })}
      >
        <div className='p-4'>
          <TranslationPreferences
            languages={targetLanguages}
            targetLanguage={config.target_language}
            instructions={config.instructions ?? ''}
            onTargetLanguageChange={(target_language) => onChange({ ...config, target_language })}
            onInstructionsChange={(instructions) => onChange({ ...config, instructions })}
          />
        </div>
      </SettingsGroup>
    </SettingsPage>
  )
}

function credentialField(provider: Providers['provider']): keyof TranslationCredentialsView | null {
  return provider === 'local' ? null : provider
}

function TranslationPreferences({
  languages,
  targetLanguage,
  instructions,
  onTargetLanguageChange,
  onInstructionsChange,
}: {
  languages: TargetLanguageView[]
  targetLanguage: string
  instructions: string
  onTargetLanguageChange: (language: string) => void
  onInstructionsChange: (instructions: string) => void
}) {
  const { t, i18n } = useTranslation()
  const displayNames = new Intl.DisplayNames([i18n.resolvedLanguage ?? i18n.language], {
    type: 'language',
  })

  return (
    <div className='grid gap-4 sm:grid-cols-[minmax(0,0.8fr)_minmax(0,1.2fr)]'>
      <div className='grid content-start gap-1.5'>
        <Label htmlFor='translation-target-language' className='text-xs font-medium'>
          {t('native.model.targetLanguage', { defaultValue: 'Target language' })}
        </Label>
        <Select
          value={normalizeTargetLanguage(targetLanguage, languages)}
          disabled={languages.length === 0}
          onValueChange={onTargetLanguageChange}
        >
          <SelectTrigger id='translation-target-language' className='w-full bg-background'>
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            {languages.map((language) => (
              <SelectItem key={language.tag} value={language.tag}>
                {displayNames.of(language.tag) ?? language.name}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
      </div>
      <div className='grid gap-1.5'>
        <Label htmlFor='translation-instructions' className='text-xs font-medium'>
          {t('native.model.instructions', { defaultValue: 'Instructions' })}
        </Label>
        <Textarea
          id='translation-instructions'
          className='min-h-16 resize-y text-[12px] md:text-[12px]'
          value={instructions}
          placeholder={t('native.model.instructionsPlaceholder', {
            defaultValue: 'Optional guidance for tone, names, and formatting.',
          })}
          onChange={(event) => onInstructionsChange(event.currentTarget.value)}
        />
      </div>
    </div>
  )
}

function hasPipelineModelOptions(model: PipelineModel): boolean {
  switch (model.model) {
    case 'paddleocr-vl-1.6':
    case 'manga-ocr':
    case 'baberu-ocr':
      return false
    default:
      return true
  }
}

function PipelineModelFields({
  model,
  onChange,
}: {
  model: PipelineModel
  onChange: (model: PipelineModel) => void
}) {
  const { t } = useTranslation()
  switch (model.model) {
    case 'koharu-layout-rfdetr-seg-2xl':
      return (
        <div className='grid gap-2 sm:grid-cols-2'>
          <OptionalNumberSetting
            label='Text threshold'
            value={model.text_threshold ?? null}
            min={0}
            max={1}
            step={0.05}
            onChange={(text_threshold) => onChange({ ...model, text_threshold })}
          />
          <OptionalNumberSetting
            label='Bubble threshold'
            value={model.bubble_threshold ?? null}
            min={0}
            max={1}
            step={0.05}
            onChange={(bubble_threshold) => onChange({ ...model, bubble_threshold })}
          />
          <OptionalNumberSetting
            label='Panel threshold'
            value={model.panel_threshold ?? null}
            min={0}
            max={1}
            step={0.05}
            onChange={(panel_threshold) => onChange({ ...model, panel_threshold })}
          />
        </div>
      )
    case 'paddleocr-vl-1.6':
    case 'manga-ocr':
    case 'baberu-ocr':
      return null
    case 'aot-inpainting':
      return (
        <div className='max-w-48'>
          <NumberSetting
            label={t('native.model.maxSide', { defaultValue: 'Maximum side' })}
            value={model.max_side ?? 2048}
            min={1}
            step={1}
            onChange={(max_side) => onChange({ ...model, max_side })}
          />
        </div>
      )
    case 'lama':
      return (
        <div className='grid gap-2 sm:grid-cols-2'>
          <label className='grid gap-0.5 text-xs'>
            <span>HD strategy</span>
            <Select
              value={model.hd_strategy ?? 'crop'}
              onValueChange={(hd_strategy) =>
                onChange({ ...model, hd_strategy: hd_strategy as LaMaHDStrategy })
              }
            >
              <SelectTrigger>
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                <SelectItem value='original'>Original</SelectItem>
                <SelectItem value='resize'>Resize</SelectItem>
                <SelectItem value='crop'>Crop</SelectItem>
              </SelectContent>
            </Select>
          </label>
          <NumberSetting
            label='Crop trigger size'
            value={model.hd_strategy_crop_trigger_size ?? 800}
            min={1}
            step={1}
            onChange={(hd_strategy_crop_trigger_size) =>
              onChange({ ...model, hd_strategy_crop_trigger_size })
            }
          />
          <NumberSetting
            label='Crop margin'
            value={model.hd_strategy_crop_margin ?? 128}
            min={0}
            step={1}
            onChange={(hd_strategy_crop_margin) => onChange({ ...model, hd_strategy_crop_margin })}
          />
          <NumberSetting
            label='Resize limit'
            value={model.hd_strategy_resize_limit ?? 1280}
            min={1}
            step={1}
            onChange={(hd_strategy_resize_limit) =>
              onChange({ ...model, hd_strategy_resize_limit })
            }
          />
          <BooleanSetting
            label='Keep unmasked area'
            value={model.keep_unmasked_area ?? true}
            onChange={(keep_unmasked_area) => onChange({ ...model, keep_unmasked_area })}
          />
        </div>
      )
    case 'flux2-klein':
      return (
        <div className='grid gap-2 sm:grid-cols-2'>
          <TextSetting
            label='Prompt'
            value={model.prompt ?? 'Remove the text and reconstruct the background.'}
            onChange={(prompt) => onChange({ ...model, prompt })}
          />
          <OptionalNumberSetting
            label='Mask crop padding'
            value={model.padding_mask_crop ?? null}
            min={0}
            step={1}
            onChange={(padding_mask_crop) => onChange({ ...model, padding_mask_crop })}
          />
          <NumberSetting
            label='Strength'
            value={model.strength ?? 0.8}
            min={0.01}
            max={1}
            step={0.01}
            onChange={(strength) => onChange({ ...model, strength })}
          />
          <NumberSetting
            label='Inference steps'
            value={model.num_inference_steps ?? 4}
            min={1}
            step={1}
            onChange={(num_inference_steps) => onChange({ ...model, num_inference_steps })}
          />
          <NumberSetting
            label='Seed'
            value={model.seed ?? -1}
            step={1}
            onChange={(seed) => onChange({ ...model, seed })}
          />
        </div>
      )
    case 'rorem-mixed':
      return (
        <div className='grid gap-2 sm:grid-cols-2'>
          <TextSetting
            label='Prompt'
            value={model.prompt ?? ''}
            onChange={(prompt) => onChange({ ...model, prompt })}
          />
          <TextSetting
            label='Negative prompt'
            value={model.negative_prompt ?? ''}
            onChange={(negative_prompt) => onChange({ ...model, negative_prompt })}
          />
          <NumberSetting
            label='Resolution'
            value={model.resolution ?? 512}
            min={512}
            max={1024}
            step={512}
            onChange={(resolution) => onChange({ ...model, resolution })}
          />
          <NumberSetting
            label='Mask dilation'
            value={model.mask_dilation ?? 0}
            min={0}
            max={255}
            step={1}
            onChange={(mask_dilation) => onChange({ ...model, mask_dilation })}
          />
          <NumberSetting
            label='Inference steps'
            value={model.num_inference_steps ?? 30}
            min={1}
            step={1}
            onChange={(num_inference_steps) => onChange({ ...model, num_inference_steps })}
          />
          <NumberSetting
            label='Guidance scale'
            value={model.guidance_scale ?? 8}
            min={0.01}
            step={0.1}
            onChange={(guidance_scale) => onChange({ ...model, guidance_scale })}
          />
          <NumberSetting
            label='Strength'
            value={model.strength ?? 0.999}
            min={0.001}
            max={0.999}
            step={0.001}
            onChange={(strength) => onChange({ ...model, strength })}
          />
          <NumberSetting
            label='Seed'
            value={model.seed ?? -1}
            step={1}
            onChange={(seed) => onChange({ ...model, seed })}
          />
        </div>
      )
  }
}

function hasProviderOptions(model: Providers): boolean {
  return model.provider !== 'google_cloud_translation' && model.provider !== 'caiyun'
}

function ProviderFields({
  model,
  localModels,
  onChange,
}: {
  model: Providers
  localModels: string[]
  onChange: (model: Providers) => void
}) {
  const { t } = useTranslation()
  switch (model.provider) {
    case 'local':
      return (
        <div className='grid gap-0.5'>
          <Label htmlFor='local-translation-model' className='text-xs font-normal'>
            {t('native.model.localModel', { defaultValue: 'Local model' })}
          </Label>
          <Select
            value={model.model}
            onValueChange={(value) => onChange({ ...model, model: value })}
          >
            <SelectTrigger id='local-translation-model' className='w-full bg-background'>
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              {localModels.map((localModel) => (
                <SelectItem key={localModel} value={localModel}>
                  {localModel}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
        </div>
      )
    case 'openai':
    case 'gemini':
    case 'claude':
    case 'deepseek':
    case 'openrouter':
      return <ChatFields model={model} onChange={onChange} />
    case 'lm_studio':
      return (
        <div className='grid gap-2 sm:grid-cols-2'>
          <TextSetting
            label={t('native.model.baseUrl', { defaultValue: 'Base URL' })}
            type='url'
            value={model.base_url}
            onChange={(base_url) => onChange({ ...model, base_url })}
          />
          <TextSetting
            label={t('native.model.remoteModel', { defaultValue: 'Remote model' })}
            value={model.model}
            onChange={(value) => onChange({ ...model, model: value })}
          />
          <OptionalNumberSetting
            label={t('native.model.temperature', { defaultValue: 'Temperature' })}
            value={model.temperature}
            min={0}
            max={1}
            onChange={(temperature) => onChange({ ...model, temperature })}
          />
          <OptionalNumberSetting
            label={t('native.model.maxTokens', { defaultValue: 'Maximum tokens' })}
            value={model.max_tokens}
            min={1}
            onChange={(max_tokens) => onChange({ ...model, max_tokens })}
          />
          <BooleanSetting
            label={t('native.model.thinking', { defaultValue: 'Thinking' })}
            value={model.thinking}
            onChange={(thinking) => onChange({ ...model, thinking })}
          />
        </div>
      )
    case 'openai_compatible':
      return (
        <div className='grid gap-2 sm:grid-cols-2'>
          <TextSetting
            label={t('native.model.baseUrl', { defaultValue: 'Base URL' })}
            type='url'
            value={model.base_url}
            onChange={(base_url) => onChange({ ...model, base_url })}
          />
          <TextSetting
            label={t('native.model.remoteModel', { defaultValue: 'Remote model' })}
            value={model.model}
            onChange={(value) => onChange({ ...model, model: value })}
          />
          <OptionalNumberSetting
            label={t('native.model.temperature', { defaultValue: 'Temperature' })}
            value={model.temperature}
            min={0}
            onChange={(temperature) => onChange({ ...model, temperature })}
          />
          <OptionalNumberSetting
            label={t('native.model.maxTokens', { defaultValue: 'Maximum tokens' })}
            value={model.max_tokens}
            min={1}
            onChange={(max_tokens) => onChange({ ...model, max_tokens })}
          />
        </div>
      )
    case 'deepl':
      return (
        <TextSetting
          label={t('native.model.baseUrl', { defaultValue: 'Base URL' })}
          type='url'
          value={model.base_url ?? ''}
          onChange={(base_url) => onChange({ ...model, base_url: base_url || null })}
        />
      )
    case 'google_cloud_translation':
    case 'caiyun':
      return null
  }
}

function ChatFields({
  model,
  onChange,
}: {
  model: Extract<
    Providers,
    { provider: 'openai' | 'gemini' | 'claude' | 'deepseek' | 'openrouter' }
  >
  onChange: (model: Providers) => void
}) {
  const { t } = useTranslation()
  return (
    <div className='grid gap-2 sm:grid-cols-2'>
      <TextSetting
        label={t('native.model.remoteModel', { defaultValue: 'Remote model' })}
        value={model.model}
        onChange={(value) => onChange({ ...model, model: value })}
      />
      <OptionalNumberSetting
        label={t('native.model.temperature', { defaultValue: 'Temperature' })}
        value={model.temperature}
        min={0}
        onChange={(temperature) => onChange({ ...model, temperature })}
      />
      <OptionalNumberSetting
        label={t('native.model.maxTokens', { defaultValue: 'Maximum tokens' })}
        value={model.max_tokens}
        min={1}
        onChange={(max_tokens) => onChange({ ...model, max_tokens })}
      />
      <BooleanSetting
        label={t('native.model.thinking', { defaultValue: 'Thinking' })}
        value={model.thinking}
        onChange={(thinking) => onChange({ ...model, thinking })}
      />
    </div>
  )
}

function SettingRow({ label, children }: { label: string; children: React.ReactNode }) {
  return (
    <div className='flex items-center justify-between gap-3'>
      <Label className='text-xs font-normal'>{label}</Label>
      {children}
    </div>
  )
}

function ShortcutSettings() {
  const { t } = useTranslation()
  const shortcuts = useEditorStore((state) => state.shortcuts)
  const setShortcut = useEditorStore((state) => state.setShortcut)
  const actions: ShortcutAction[] = ['select', 'text', 'text_mask', 'brush_mask', 'pan', 'fit']
  return (
    <Section
      title={t('native.settings.shortcuts', { defaultValue: 'Shortcuts' })}
      description={t('native.settings.shortcutsHelp', {
        defaultValue: 'Single-key tool shortcuts are stored on this device.',
      })}
    >
      <div className='divide-y divide-border overflow-hidden rounded-xl border border-border bg-card'>
        {actions.map((action) => (
          <div key={action} className='flex items-center justify-between gap-3 px-4 py-2'>
            <span className='text-sm'>
              {action === 'fit'
                ? t('native.canvas.fit', { defaultValue: 'Fit Window' })
                : t(`native.tools.${action}`, { defaultValue: action })}
            </span>
            <Input
              className='h-8 w-14 text-center uppercase'
              maxLength={1}
              value={shortcuts[action]}
              onChange={(event) => setShortcut(action, event.currentTarget.value)}
            />
          </div>
        ))}
      </div>
    </Section>
  )
}

function TextSetting({
  label,
  value,
  type = 'text',
  onChange,
}: {
  label: string
  value: string
  type?: 'text' | 'url'
  onChange: (value: string) => void
}) {
  return (
    <label className='grid gap-0.5 text-xs'>
      <span>{label}</span>
      <Input
        type={type}
        value={value}
        required={type === 'url'}
        onChange={(event) => onChange(event.currentTarget.value)}
      />
    </label>
  )
}

function NumberSetting({
  label,
  value,
  min,
  max,
  step,
  onChange,
}: {
  label: string
  value: number
  min?: number
  max?: number
  step?: number
  onChange: (value: number) => void
}) {
  return (
    <label className='grid gap-0.5 text-xs'>
      <span>{label}</span>
      <Input
        type='number'
        value={displayNumber(value, step)}
        min={min}
        max={max}
        step={step}
        onChange={(event) => {
          const next = Number(event.currentTarget.value)
          if (Number.isFinite(next)) onChange(next)
        }}
      />
    </label>
  )
}

function OptionalNumberSetting({
  label,
  value,
  min,
  max,
  step,
  onChange,
}: {
  label: string
  value: number | null
  min?: number
  max?: number
  step?: number
  onChange: (value: number | null) => void
}) {
  const { t } = useTranslation()
  return (
    <label className='grid gap-0.5 text-xs'>
      <span>{label}</span>
      <Input
        type='number'
        value={value === null ? '' : displayNumber(value, step)}
        min={min}
        max={max}
        step={step}
        placeholder={t('native.model.default', { defaultValue: 'Default' })}
        onChange={(event) =>
          onChange(event.currentTarget.value === '' ? null : Number(event.currentTarget.value))
        }
      />
    </label>
  )
}

function displayNumber(value: number, step?: number): number {
  if (!step || !Number.isFinite(value)) return value
  const precision = Math.max(0, -Math.floor(Math.log10(step)))
  return Number(value.toFixed(precision))
}

function BooleanSetting({
  label,
  value,
  onChange,
}: {
  label: string
  value: boolean
  onChange: (value: boolean) => void
}) {
  return (
    <SettingRow label={label}>
      <Switch checked={value} onCheckedChange={onChange} />
    </SettingRow>
  )
}

function defaultPipelineModel(model: PipelineModel['model']): PipelineModel {
  switch (model) {
    case 'koharu-layout-rfdetr-seg-2xl':
      return {
        model,
        text_threshold: null,
        bubble_threshold: null,
        panel_threshold: null,
      }
    case 'paddleocr-vl-1.6':
      return { model }
    case 'manga-ocr':
      return { model }
    case 'baberu-ocr':
      return { model }
    case 'lama':
      return {
        model,
        hd_strategy: 'crop',
        hd_strategy_crop_trigger_size: 800,
        hd_strategy_crop_margin: 128,
        hd_strategy_resize_limit: 1280,
        keep_unmasked_area: true,
      }
    case 'aot-inpainting':
      return { model, max_side: 2048 }
    case 'flux2-klein':
      return {
        model,
        prompt: 'Remove the text and reconstruct the background.',
        padding_mask_crop: null,
        strength: 0.8,
        num_inference_steps: 4,
        seed: -1,
      }
    case 'rorem-mixed':
      return { model }
  }
}

function setPhaseModel(
  config: PipelineConfig,
  phase: ModelPhase,
  model: PipelineModel,
): PipelineConfig {
  switch (phase) {
    case 'detection':
      return { ...config, detection: model as DetectionModel }
    case 'ocr':
      return { ...config, ocr: model as OcrModel }
    case 'inpainting':
      return { ...config, inpainting: model as InpaintingModel }
  }
}
