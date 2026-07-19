'use client'

import { Cpu, Eye, EyeOff, Keyboard, Monitor, Moon, Palette, Save, Sun } from 'lucide-react'
import { useTheme } from 'next-themes'
import { useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'

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
  useEditorStore,
  type PipelineConfig,
  type ProcessorConfig,
  type ShortcutAction,
  type Phase,
  type TargetLanguageView,
  type TranslationCredentialsView,
  type TranslationSettings,
  type Providers,
} from '@/lib/koharu'

const settingsTabs = [
  { id: 'appearance', icon: Palette, label: 'native.settings.appearance' },
  { id: 'pipeline', icon: Cpu, label: 'native.settings.pipeline' },
  { id: 'shortcuts', icon: Keyboard, label: 'native.settings.shortcuts' },
] as const
type SettingsTab = (typeof settingsTabs)[number]['id']

type PipelineModel = ProcessorConfig
type ModelName = PipelineModel['model'] | Providers['provider']
const phases: Phase[] = [
  'detection',
  'segmentation',
  'ocr',
  'translation',
  'typography',
  'inpainting',
]
type PipelinePhase = Exclude<Phase, 'translation'>
const modelOptions = {
  detection: ['pp_doclayout_v3', 'comic_text_detector', 'koharu_yolo26s', 'comic_onomatopoeia'],
  segmentation: ['manga_text_mask', 'speech_bubble_yolov8m', 'mask_fusion'],
  ocr: ['paddleocr_vl_1.6', 'manga_ocr', 'baberu_ocr'],
  translation: [
    'local',
    'openai',
    'gemini',
    'claude',
    'deepseek',
    'openai_compatible',
    'openrouter',
    'lm_studio',
    'deepl',
    'google_cloud_translation',
    'caiyun',
  ],
  typography: ['font_detector'],
  inpainting: ['lama', 'aot_inpainting', 'flux2_klein'],
} satisfies Record<Phase, ModelName[]>
const processorOrder: PipelineModel['model'][] = [
  'pp_doclayout_v3',
  'comic_text_detector',
  'koharu_yolo26s',
  'manga_text_mask',
  'speech_bubble_yolov8m',
  'comic_onomatopoeia',
  'mask_fusion',
  'paddleocr_vl_1.6',
  'manga_ocr',
  'baberu_ocr',
  'font_detector',
  'lama',
  'aot_inpainting',
  'flux2_klein',
]
const modelLabels: Record<ModelName, string> = {
  comic_text_detector: 'Comic Text Detector',
  pp_doclayout_v3: 'PP-DocLayoutV3',
  koharu_yolo26s: 'Koharu YOLO26s Layout',
  comic_onomatopoeia: 'COO Detector + OCR',
  mask_fusion: 'Semantic Mask Fusion',
  manga_text_mask: 'Manga Text Mask',
  speech_bubble_yolov8m: 'Speech Bubble (YOLOv8m)',
  'paddleocr_vl_1.6': 'PaddleOCR-VL 1.6',
  manga_ocr: 'Manga OCR',
  baberu_ocr: 'Baberu OCR',
  local: 'Local',
  openai: 'OpenAI',
  gemini: 'Gemini',
  claude: 'Claude',
  deepseek: 'DeepSeek',
  openai_compatible: 'OpenAI-compatible',
  openrouter: 'OpenRouter',
  lm_studio: 'LM Studio',
  deepl: 'DeepL',
  google_cloud_translation: 'Google Cloud Translation',
  caiyun: 'Caiyun',
  font_detector: 'Font Detector',
  lama: 'LaMa',
  aot_inpainting: 'AOT Inpainting',
  flux2_klein: 'FLUX.2 Klein',
}
const phaseDescriptions: Record<Phase, string> = {
  detection: 'Locate text on the page.',
  segmentation: 'Refine the areas that should be cleaned.',
  ocr: 'Read the text inside each region.',
  translation: 'Convert source text to the target language.',
  typography: 'Choose fonts and fit translated text.',
  inpainting: 'Rebuild the artwork behind removed text.',
}

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
      <DialogContent className='flex h-[600px] max-h-[85vh] w-[760px] max-w-[92vw] flex-col gap-0 overflow-hidden p-0'>
        <DialogTitle className='sr-only'>
          {t('native.settings.title', { defaultValue: 'Settings' })}
        </DialogTitle>
        <DialogDescription className='sr-only'>
          {t('native.settings.description', { defaultValue: 'Koharu settings' })}
        </DialogDescription>

        <div className='flex min-h-0 flex-1'>
          <nav className='flex w-[180px] shrink-0 flex-col gap-1 border-r border-border bg-muted/30 p-3'>
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
              <div className='p-6'>
                {tab === 'appearance' && (
                  <AppearanceSettings theme={theme ?? 'system'} onThemeChange={setTheme} />
                )}

                {tab === 'pipeline' && (
                  <Section
                    title={t('native.settings.pipeline', { defaultValue: 'Pipeline' })}
                    description={t('native.settings.pipelineHelp', {
                      defaultValue:
                        'Choose how Koharu detects, reads, translates, and rebuilds each page.',
                    })}
                  >
                    {draft && translationDraft ? (
                      <div className='divide-y divide-border'>
                        {phases.map((phase) =>
                          phase === 'translation' ? (
                            <TranslationEditor
                              key={phase}
                              config={translationDraft}
                              localModels={settings?.local_translation_models ?? []}
                              targetLanguages={settings?.target_languages ?? []}
                              onChange={setTranslationDraft}
                            />
                          ) : (
                            <PhaseEditor
                              key={phase}
                              phase={phase}
                              config={draft}
                              onChange={setDraft}
                            />
                          ),
                        )}
                      </div>
                    ) : (
                      <div className='rounded-xl border p-4 text-xs text-muted-foreground'>
                        {t('native.settings.unavailable', {
                          defaultValue: 'Settings are unavailable while disconnected.',
                        })}
                      </div>
                    )}
                  </Section>
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

function PhaseEditor({
  phase,
  config,
  onChange,
}: {
  phase: PipelinePhase
  config: PipelineConfig
  onChange: (config: PipelineConfig) => void
}) {
  const { t } = useTranslation()
  return (
    <article className='grid grid-cols-[minmax(8rem,150px)_minmax(0,1fr)] gap-5 py-3 first:pt-0 last:pb-0'>
      <div className='min-w-0 pt-1'>
        <Label className='text-xs leading-none font-semibold'>
          {t(`native.phase.${phase}`, { defaultValue: phase })}
        </Label>
        <p
          id={`pipeline-${phase}-description`}
          className='mt-1 text-[10px] leading-snug text-muted-foreground'
        >
          {t(`native.phaseDescription.${phase}`, { defaultValue: phaseDescriptions[phase] })}
        </p>
      </div>
      <div className='min-w-0 space-y-2' aria-describedby={`pipeline-${phase}-description`}>
        {modelOptions[phase].map((name) => {
          const index = config.processors.findIndex((processor) => processor.model === name)
          const model = index >= 0 ? config.processors[index] : null
          return (
            <div key={name} className='rounded-lg border border-border bg-background p-2.5'>
              <div className='flex items-center justify-between gap-3'>
                <Label className='text-xs font-medium'>{modelLabels[name]}</Label>
                <Switch
                  checked={model !== null}
                  aria-label={modelLabels[name]}
                  onCheckedChange={(enabled) =>
                    onChange(
                      enabled
                        ? {
                            ...config,
                            processors: insertPipelineModel(config.processors, name),
                          }
                        : {
                            ...config,
                            processors: config.processors.filter(
                              (processor) => processor.model !== name,
                            ),
                          },
                    )
                  }
                />
              </div>
              {model && hasPipelineModelOptions(model) && (
                <div className='mt-2 border-t border-border pt-2'>
                  <PipelineModelFields
                    model={model}
                    onChange={(next) =>
                      onChange({
                        ...config,
                        processors: config.processors.map((processor, processorIndex) =>
                          processorIndex === index ? next : processor,
                        ),
                      })
                    }
                  />
                </div>
              )}
            </div>
          )
        })}
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
  const value = credential ? config.credentials[credential] : null
  const configured = Boolean(value)
  const [revealCredential, setRevealCredential] = useState(false)

  useEffect(() => setRevealCredential(false), [credential])

  return (
    <article className='grid grid-cols-[minmax(8rem,150px)_minmax(0,1fr)] gap-5 py-3 first:pt-0 last:pb-0'>
      <div className='min-w-0 pt-1'>
        <Label htmlFor='pipeline-translation' className='text-xs leading-none font-semibold'>
          {t('native.phase.translation', { defaultValue: 'translation' })}
        </Label>
        <p
          id='pipeline-translation-description'
          className='mt-1 text-[10px] leading-snug text-muted-foreground'
        >
          {t('native.phaseDescription.translation', {
            defaultValue: phaseDescriptions.translation,
          })}
        </p>
      </div>
      <div className='min-w-0 space-y-2'>
        <Select
          value={config.model.provider}
          onValueChange={(provider) => replace(defaultProvider(provider as Providers['provider']))}
        >
          <SelectTrigger
            id='pipeline-translation'
            aria-describedby='pipeline-translation-description'
            className='w-full bg-background'
          >
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            {modelOptions.translation.map((option) => (
              <SelectItem key={option} value={option}>
                {modelLabels[option]}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
        {hasProviderOptions(config.model) && (
          <ProviderFields model={config.model} localModels={localModels} onChange={replace} />
        )}
        {credential && (
          <div className='grid gap-0.5'>
            <Label htmlFor={`translation-credential-${credential}`} className='text-xs font-normal'>
              {t('native.settings.credentials', { defaultValue: 'Credential' })}
            </Label>
            <div className='flex gap-2'>
              <Input
                id={`translation-credential-${credential}`}
                aria-label={`${credential.replaceAll('_', ' ')} credential`}
                type={revealCredential ? 'text' : 'password'}
                autoComplete='new-password'
                className='[&::-ms-reveal]:hidden'
                value={value ?? ''}
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
                      [credential]: event.currentTarget.value,
                    },
                  })
                }
              />
              <Button
                type='button'
                size='icon-sm'
                variant='outline'
                disabled={!configured}
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
                      credentials: { ...config.credentials, [credential]: '' },
                    })
                  }
                >
                  {t('native.settings.clear', { defaultValue: 'Clear' })}
                </Button>
              )}
            </div>
          </div>
        )}
        <TranslationPreferences
          languages={targetLanguages}
          targetLanguage={config.target_language}
          instructions={config.instructions ?? ''}
          onTargetLanguageChange={(target_language) => onChange({ ...config, target_language })}
          onInstructionsChange={(instructions) => onChange({ ...config, instructions })}
        />
      </div>
    </article>
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
    <div className='mt-3 grid gap-2 border-t border-border/70 pt-3'>
      <div className='grid gap-0.5'>
        <Label htmlFor='translation-target-language' className='text-xs font-normal'>
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
      <div className='grid gap-0.5'>
        <Label htmlFor='translation-instructions' className='text-xs font-normal'>
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

function normalizeTargetLanguage(value: string, languages: TargetLanguageView[]): string {
  if (languages.some((language) => language.tag === value)) return value
  return languages.find((language) => language.name === value)?.tag ?? languages[0]?.tag ?? value
}

function hasPipelineModelOptions(model: PipelineModel): boolean {
  switch (model.model) {
    case 'comic_text_detector':
    case 'paddleocr_vl_1.6':
    case 'manga_ocr':
    case 'baberu_ocr':
    case 'lama':
    case 'flux2_klein':
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
    case 'comic_text_detector':
    case 'paddleocr_vl_1.6':
    case 'manga_ocr':
    case 'baberu_ocr':
    case 'lama':
    case 'flux2_klein':
      return null
    case 'koharu_yolo26s':
      return (
        <div className='grid gap-2 sm:grid-cols-2'>
          <NumberSetting
            label={t('native.model.confidence', { defaultValue: 'Confidence' })}
            value={model.confidence ?? 0.25}
            min={0}
            max={1}
            step={0.05}
            onChange={(confidence) => onChange({ ...model, confidence })}
          />
          <BooleanSetting
            label='Dialogue regions'
            value={model.dialogue_regions ?? false}
            onChange={(dialogue_regions) => onChange({ ...model, dialogue_regions })}
          />
          <BooleanSetting
            label='COO proposals'
            value={model.onomatopoeia_regions ?? true}
            onChange={(onomatopoeia_regions) => onChange({ ...model, onomatopoeia_regions })}
          />
          <BooleanSetting
            label='Instance text masks'
            value={model.text_masks ?? true}
            onChange={(text_masks) => onChange({ ...model, text_masks })}
          />
        </div>
      )
    case 'comic_onomatopoeia':
      return (
        <div className='grid gap-2 sm:grid-cols-3'>
          <NumberSetting
            label='COO confidence'
            value={model.onomatopoeia_threshold ?? 0.5}
            min={0}
            max={1}
            step={0.05}
            onChange={(onomatopoeia_threshold) => onChange({ ...model, onomatopoeia_threshold })}
          />
          <NumberSetting
            label='OCR confidence'
            value={model.ocr_threshold ?? 0.5}
            min={0}
            max={1}
            step={0.05}
            onChange={(ocr_threshold) => onChange({ ...model, ocr_threshold })}
          />
          <NumberSetting
            label='Dedup IoU'
            value={model.dedup_iou ?? 0.3}
            min={0}
            max={1}
            step={0.05}
            onChange={(dedup_iou) => onChange({ ...model, dedup_iou })}
          />
        </div>
      )
    case 'mask_fusion':
      return (
        <NumberSetting
          label='COO region padding'
          value={model.coo_padding ?? 4}
          min={0}
          step={1}
          onChange={(coo_padding) => onChange({ ...model, coo_padding })}
        />
      )
    case 'pp_doclayout_v3':
      return (
        <NumberSetting
          label={t('native.model.confidence', { defaultValue: 'Confidence' })}
          value={model.confidence ?? 0.25}
          min={0}
          max={1}
          step={0.05}
          onChange={(confidence) => onChange({ ...model, confidence })}
        />
      )
    case 'manga_text_mask':
      return (
        <div className='grid gap-2 sm:grid-cols-2'>
          <NumberSetting
            label={t('native.model.threshold', { defaultValue: 'Threshold' })}
            value={model.threshold ?? 0.5}
            min={0}
            max={1}
            step={0.05}
            onChange={(threshold) => onChange({ ...model, threshold })}
          />
          <OptionalNumberSetting
            label={t('native.model.maxSide', { defaultValue: 'Maximum side' })}
            value={model.max_side ?? null}
            min={1}
            onChange={(max_side) => onChange({ ...model, max_side })}
          />
          <BooleanSetting
            label={t('native.model.horizontalFlip', { defaultValue: 'Horizontal flip' })}
            value={model.horizontal_flip ?? false}
            onChange={(horizontal_flip) => onChange({ ...model, horizontal_flip })}
          />
          <BooleanSetting
            label={t('native.model.verticalFlip', { defaultValue: 'Vertical flip' })}
            value={model.vertical_flip ?? false}
            onChange={(vertical_flip) => onChange({ ...model, vertical_flip })}
          />
        </div>
      )
    case 'speech_bubble_yolov8m':
      return (
        <div className='grid gap-2 sm:grid-cols-2'>
          <OptionalNumberSetting
            label={t('native.model.confidence', { defaultValue: 'Confidence' })}
            value={model.confidence ?? null}
            min={0}
            max={1}
            step={0.05}
            onChange={(confidence) => onChange({ ...model, confidence })}
          />
          <OptionalNumberSetting
            label={t('native.model.nmsIou', { defaultValue: 'NMS IoU' })}
            value={model.nms_iou ?? null}
            min={0}
            max={1}
            step={0.05}
            onChange={(nms_iou) => onChange({ ...model, nms_iou })}
          />
        </div>
      )
    case 'font_detector':
      return (
        <NumberSetting
          label={t('native.model.topK', { defaultValue: 'Top K' })}
          value={model.top_k ?? 3}
          min={1}
          step={1}
          onChange={(top_k) => onChange({ ...model, top_k })}
        />
      )
    case 'aot_inpainting':
      return (
        <NumberSetting
          label={t('native.model.maxSide', { defaultValue: 'Maximum side' })}
          value={model.max_side ?? 2048}
          min={1}
          step={1}
          onChange={(max_side) => onChange({ ...model, max_side })}
        />
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
        value={value}
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
        value={value ?? ''}
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
    case 'comic_text_detector':
      return { model }
    case 'pp_doclayout_v3':
      return { model, confidence: 0.25 }
    case 'koharu_yolo26s':
      return {
        model,
        confidence: 0.25,
        dialogue_regions: false,
        onomatopoeia_regions: true,
        text_masks: true,
      }
    case 'comic_onomatopoeia':
      return {
        model,
        onomatopoeia_threshold: 0.5,
        ocr_threshold: 0.5,
        dedup_iou: 0.3,
      }
    case 'mask_fusion':
      return { model, coo_padding: 4 }
    case 'manga_text_mask':
      return { model, threshold: 0.5, max_side: null, horizontal_flip: false, vertical_flip: false }
    case 'speech_bubble_yolov8m':
      return { model, confidence: null, nms_iou: null }
    case 'paddleocr_vl_1.6':
      return { model }
    case 'manga_ocr':
      return { model }
    case 'baberu_ocr':
      return { model }
    case 'font_detector':
      return { model, top_k: 3 }
    case 'lama':
      return { model }
    case 'aot_inpainting':
      return { model, max_side: 2048 }
    case 'flux2_klein':
      return { model }
  }
}

function insertPipelineModel(
  processors: PipelineModel[],
  model: PipelineModel['model'],
): PipelineModel[] {
  const priority = processorOrder.indexOf(model)
  const index = processors.findIndex(
    (processor) => processorOrder.indexOf(processor.model) > priority,
  )
  const next = defaultPipelineModel(model)
  return index < 0
    ? [...processors, next]
    : [...processors.slice(0, index), next, ...processors.slice(index)]
}

function defaultProvider(provider: Providers['provider']): Providers {
  switch (provider) {
    case 'local':
      return { provider, model: 'gemma4-12b-it' }
    case 'openai':
      return {
        provider,
        model: 'gpt-4.1-mini',
        temperature: null,
        max_tokens: null,
        thinking: false,
      }
    case 'gemini':
      return {
        provider,
        model: 'gemini-2.5-flash',
        temperature: null,
        max_tokens: null,
        thinking: false,
      }
    case 'claude':
      return {
        provider,
        model: 'claude-sonnet-5',
        temperature: null,
        max_tokens: null,
        thinking: false,
      }
    case 'deepseek':
      return {
        provider,
        model: 'deepseek-v4-flash',
        temperature: null,
        max_tokens: null,
        thinking: false,
      }
    case 'openai_compatible':
      return {
        provider,
        base_url: 'http://localhost:11434/v1',
        model: 'model',
        temperature: null,
        max_tokens: null,
      }
    case 'openrouter':
      return {
        provider,
        model: 'openrouter/auto',
        temperature: null,
        max_tokens: null,
        thinking: false,
      }
    case 'lm_studio':
      return {
        provider,
        base_url: 'http://localhost:1234',
        model: 'model',
        temperature: null,
        max_tokens: null,
        thinking: false,
      }
    case 'deepl':
      return { provider, base_url: null }
    case 'google_cloud_translation':
      return { provider }
    case 'caiyun':
      return { provider }
  }
}
