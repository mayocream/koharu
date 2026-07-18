'use client'

import { Cpu, KeyRound, Keyboard, Monitor, Moon, Palette, Save, Sun } from 'lucide-react'
import { useTheme } from 'next-themes'
import { useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'

import {
  Accordion,
  AccordionContent,
  AccordionItem,
  AccordionTrigger,
} from '@/components/ui/accordion'
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
  type DetectionModel,
  type InpaintingModel,
  type OcrModel,
  type PipelineConfig,
  type SecretProvider,
  type ShortcutAction,
  type SegmentationModel,
  type Stage,
  type TargetLanguageView,
  type TranslationModel,
  type TypographyModel,
} from '@/lib/koharu'

const settingsTabs = [
  { id: 'appearance', icon: Palette, label: 'native.settings.appearance' },
  { id: 'pipeline', icon: Cpu, label: 'native.settings.pipeline' },
  { id: 'credentials', icon: KeyRound, label: 'native.settings.credentials' },
  { id: 'shortcuts', icon: Keyboard, label: 'native.settings.shortcuts' },
] as const
type SettingsTab = (typeof settingsTabs)[number]['id']

type PipelineModel =
  | DetectionModel
  | SegmentationModel
  | OcrModel
  | TranslationModel
  | TypographyModel
  | InpaintingModel
const stages: Stage[] = [
  'detection',
  'segmentation',
  'ocr',
  'translation',
  'typography',
  'inpainting',
]
const modelOptions: Record<Stage, PipelineModel['model'][]> = {
  detection: ['pp_doclayout_v3', 'comic_text_detector'],
  segmentation: ['manga_text_segmentation', 'speech_bubble_segmentation'],
  ocr: ['paddleocr_vl_1.6', 'manga_ocr'],
  translation: [
    'local',
    'openai',
    'gemini',
    'claude',
    'deepseek',
    'openai_compatible',
    'deepl',
    'google_cloud_translation',
    'caiyun',
  ],
  typography: ['font_detector'],
  inpainting: ['lama', 'aot_inpainting', 'flux2_klein'],
}
const modelLabels: Record<PipelineModel['model'], string> = {
  comic_text_detector: 'Comic Text Detector',
  pp_doclayout_v3: 'PP-DocLayoutV3',
  manga_text_segmentation: 'Manga Text Segmentation',
  speech_bubble_segmentation: 'Speech Bubble Segmentation',
  'paddleocr_vl_1.6': 'PaddleOCR-VL 1.6',
  manga_ocr: 'Manga OCR',
  local: 'Local',
  openai: 'OpenAI',
  gemini: 'Gemini',
  claude: 'Claude',
  deepseek: 'DeepSeek',
  openai_compatible: 'OpenAI-compatible',
  deepl: 'DeepL',
  google_cloud_translation: 'Google Cloud Translation',
  caiyun: 'Caiyun',
  font_detector: 'Font Detector',
  lama: 'LaMa',
  aot_inpainting: 'AOT Inpainting',
  flux2_klein: 'FLUX.2 Klein',
}
const stageDescriptions: Record<Stage, string> = {
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
  const targetLanguage = useEditorStore((state) => state.targetLanguage)
  const setTargetLanguage = useEditorStore((state) => state.setTargetLanguage)
  const instructions = useEditorStore((state) => state.instructions)
  const setInstructions = useEditorStore((state) => state.setInstructions)
  const [draft, setDraft] = useState<PipelineConfig | null>(settings?.pipeline ?? null)
  const [targetLanguageDraft, setTargetLanguageDraft] = useState(targetLanguage)
  const [instructionsDraft, setInstructionsDraft] = useState(instructions)
  const [secrets, setSecrets] = useState<Partial<Record<SecretProvider, string>>>({})
  const [tab, setTab] = useState<SettingsTab>('appearance')

  useEffect(() => {
    if (open) {
      setTab('appearance')
      setTargetLanguageDraft(targetLanguage)
      setInstructionsDraft(instructions)
      koharuClient.fire({ type: 'get_settings' })
    }
  }, [instructions, open, targetLanguage])
  useEffect(() => {
    setDraft(settings?.pipeline ?? null)
    if (open && settings) {
      setTargetLanguageDraft((current) =>
        normalizeTargetLanguage(current, settings.target_languages),
      )
    }
  }, [open, settings])

  const save = () => {
    if (!draft) return
    koharuClient
      .command({ type: 'set_pipeline_config', config: draft })
      .then((result) => {
        if (result === 'accepted') {
          setTargetLanguage(targetLanguageDraft)
          setInstructions(instructionsDraft)
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
                    {draft ? (
                      <div className='divide-y divide-border'>
                        {stages.map((stage) => (
                          <StageEditor
                            key={stage}
                            stage={stage}
                            config={draft}
                            localModels={settings?.local_translation_models ?? []}
                            targetLanguages={settings?.target_languages ?? []}
                            targetLanguage={targetLanguageDraft}
                            instructions={instructionsDraft}
                            onTargetLanguageChange={setTargetLanguageDraft}
                            onInstructionsChange={setInstructionsDraft}
                            onChange={setDraft}
                          />
                        ))}
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

                {tab === 'credentials' && (
                  <Section
                    title={t('native.settings.credentials', { defaultValue: 'Credentials' })}
                    description={t('native.settings.credentialsHelp', {
                      defaultValue: 'Secret values are write-only and never returned to the UI.',
                    })}
                  >
                    <Accordion type='multiple' className='-mx-1'>
                      {settings?.credentials.map(({ provider, configured }) => (
                        <AccordionItem key={provider} value={provider} className='border-border'>
                          <AccordionTrigger className='px-1 py-3 hover:no-underline'>
                            <div className='flex flex-1 items-center gap-2.5 pr-2'>
                              <span
                                className={`size-2 shrink-0 rounded-full ${configured ? 'bg-green-500' : 'bg-muted-foreground/35'}`}
                              />
                              <span className='text-sm font-medium capitalize'>
                                {provider.replaceAll('_', ' ')}
                              </span>
                              <span className='ml-auto text-xs font-normal text-muted-foreground'>
                                {configured
                                  ? t('native.settings.configured', {
                                      defaultValue: 'Configured',
                                    })
                                  : t('native.settings.notConfigured', {
                                      defaultValue: 'Not configured',
                                    })}
                              </span>
                            </div>
                          </AccordionTrigger>
                          <AccordionContent className='space-y-2 px-1 pt-1 pb-4'>
                            <Label className='text-xs' htmlFor={`secret-${provider}`}>
                              {t('native.settings.credentials', { defaultValue: 'Credential' })}
                            </Label>
                            <div className='flex gap-2'>
                              <Input
                                id={`secret-${provider}`}
                                type='password'
                                autoComplete='new-password'
                                className='[&::-ms-reveal]:hidden'
                                placeholder={
                                  configured
                                    ? t('native.settings.configured', {
                                        defaultValue: 'Configured',
                                      })
                                    : t('native.settings.notConfigured', {
                                        defaultValue: 'Not configured',
                                      })
                                }
                                value={secrets[provider] ?? ''}
                                onChange={(event) =>
                                  setSecrets((current) => ({
                                    ...current,
                                    [provider]: event.currentTarget.value,
                                  }))
                                }
                                onKeyDown={(event) => {
                                  if (event.key !== 'Enter') return
                                  const value = secrets[provider]?.trim()
                                  if (!value) return
                                  koharuClient.fire({ type: 'set_secret', provider, value })
                                  setSecrets((current) => ({ ...current, [provider]: '' }))
                                }}
                              />
                              <Button
                                size='sm'
                                disabled={!secrets[provider]?.trim()}
                                onClick={() => {
                                  const value = secrets[provider]?.trim()
                                  if (!value) return
                                  koharuClient.fire({ type: 'set_secret', provider, value })
                                  setSecrets((current) => ({ ...current, [provider]: '' }))
                                }}
                              >
                                {t('native.settings.set', { defaultValue: 'Set' })}
                              </Button>
                              {configured && (
                                <Button
                                  size='sm'
                                  variant='destructive'
                                  onClick={() =>
                                    koharuClient.fire({
                                      type: 'set_secret',
                                      provider,
                                      value: null,
                                    })
                                  }
                                >
                                  {t('native.settings.clear', { defaultValue: 'Clear' })}
                                </Button>
                              )}
                            </div>
                          </AccordionContent>
                        </AccordionItem>
                      ))}
                    </Accordion>
                  </Section>
                )}

                {tab === 'shortcuts' && <ShortcutSettings />}
              </div>
            </ScrollArea>

            <div className='flex justify-end gap-2 border-t px-5 py-3'>
              <Button variant='outline' onClick={() => setOpen(false)}>
                {t('common.cancel', { defaultValue: 'Cancel' })}
              </Button>
              <Button disabled={!draft} onClick={save}>
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

function StageEditor({
  stage,
  config,
  localModels,
  targetLanguages,
  targetLanguage,
  instructions,
  onTargetLanguageChange,
  onInstructionsChange,
  onChange,
}: {
  stage: Stage
  config: PipelineConfig
  localModels: string[]
  targetLanguages: TargetLanguageView[]
  targetLanguage: string
  instructions: string
  onTargetLanguageChange: (language: string) => void
  onInstructionsChange: (instructions: string) => void
  onChange: (config: PipelineConfig) => void
}) {
  const { t } = useTranslation()
  const model = config[stage] as PipelineModel
  const replace = (next: PipelineModel) => onChange(setStage(config, stage, next))
  const hasOptions = hasModelOptions(model)
  return (
    <article className='grid grid-cols-[minmax(8rem,150px)_minmax(0,1fr)] gap-5 py-3 first:pt-0 last:pb-0'>
      <div className='min-w-0 pt-1'>
        <Label htmlFor={`pipeline-${stage}`} className='text-xs leading-none font-semibold'>
          {t(`native.stage.${stage}`, { defaultValue: stage })}
        </Label>
        <p
          id={`pipeline-${stage}-description`}
          className='mt-1 text-[10px] leading-snug text-muted-foreground'
        >
          {t(`native.stageDescription.${stage}`, { defaultValue: stageDescriptions[stage] })}
        </p>
      </div>
      <div className='min-w-0 space-y-2'>
        <Select
          value={model.model}
          onValueChange={(value) => replace(defaultModel(value as PipelineModel['model']))}
        >
          <SelectTrigger
            id={`pipeline-${stage}`}
            aria-describedby={`pipeline-${stage}-description`}
            className='w-full bg-background'
          >
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            {modelOptions[stage].map((option) => (
              <SelectItem key={option} value={option}>
                {modelLabels[option]}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
        {hasOptions && <ModelFields model={model} localModels={localModels} onChange={replace} />}
        {stage === 'translation' && (
          <TranslationPreferences
            languages={targetLanguages}
            targetLanguage={targetLanguage}
            instructions={instructions}
            onTargetLanguageChange={onTargetLanguageChange}
            onInstructionsChange={onInstructionsChange}
          />
        )}
      </div>
    </article>
  )
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

function hasModelOptions(model: PipelineModel): boolean {
  switch (model.model) {
    case 'comic_text_detector':
    case 'paddleocr_vl_1.6':
    case 'manga_ocr':
    case 'lama':
    case 'flux2_klein':
    case 'google_cloud_translation':
    case 'caiyun':
      return false
    default:
      return true
  }
}

function ModelFields({
  model,
  localModels,
  onChange,
}: {
  model: PipelineModel
  localModels: string[]
  onChange: (model: PipelineModel) => void
}) {
  const { t } = useTranslation()
  switch (model.model) {
    case 'comic_text_detector':
    case 'paddleocr_vl_1.6':
    case 'manga_ocr':
    case 'lama':
    case 'flux2_klein':
    case 'google_cloud_translation':
    case 'caiyun':
      return null
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
    case 'manga_text_segmentation':
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
    case 'speech_bubble_segmentation':
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
    case 'local':
      return (
        <div className='grid gap-0.5'>
          <Label htmlFor='local-translation-model' className='text-xs font-normal'>
            {t('native.model.localModel', { defaultValue: 'Local model' })}
          </Label>
          <Select
            value={model.local_model}
            onValueChange={(local_model) => onChange({ ...model, local_model })}
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
      return <ChatFields model={model} onChange={onChange} />
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
            value={model.remote_model}
            onChange={(remote_model) => onChange({ ...model, remote_model })}
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

function ChatFields({
  model,
  onChange,
}: {
  model: Extract<TranslationModel, { model: 'openai' | 'gemini' | 'claude' | 'deepseek' }>
  onChange: (model: PipelineModel) => void
}) {
  const { t } = useTranslation()
  return (
    <div className='grid gap-2 sm:grid-cols-2'>
      <TextSetting
        label={t('native.model.remoteModel', { defaultValue: 'Remote model' })}
        value={model.remote_model}
        onChange={(remote_model) => onChange({ ...model, remote_model })}
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

function setStage(config: PipelineConfig, stage: Stage, model: PipelineModel): PipelineConfig {
  switch (stage) {
    case 'detection':
      return { ...config, detection: model as DetectionModel }
    case 'segmentation':
      return { ...config, segmentation: model as SegmentationModel }
    case 'ocr':
      return { ...config, ocr: model as OcrModel }
    case 'translation':
      return { ...config, translation: model as TranslationModel }
    case 'typography':
      return { ...config, typography: model as TypographyModel }
    case 'inpainting':
      return { ...config, inpainting: model as InpaintingModel }
  }
}

function defaultModel(model: PipelineModel['model']): PipelineModel {
  switch (model) {
    case 'comic_text_detector':
      return { model }
    case 'pp_doclayout_v3':
      return { model, confidence: 0.25 }
    case 'manga_text_segmentation':
      return { model, threshold: 0.5, max_side: null, horizontal_flip: false, vertical_flip: false }
    case 'speech_bubble_segmentation':
      return { model, confidence: null, nms_iou: null }
    case 'paddleocr_vl_1.6':
      return { model }
    case 'manga_ocr':
      return { model }
    case 'local':
      return { model, local_model: 'lfm2.5-1.2b-instruct' }
    case 'openai':
      return { model, remote_model: 'gpt-4.1-mini', temperature: null, max_tokens: null }
    case 'gemini':
      return { model, remote_model: 'gemini-2.5-flash', temperature: null, max_tokens: null }
    case 'claude':
      return {
        model,
        remote_model: 'claude-sonnet-4-20250514',
        temperature: null,
        max_tokens: null,
      }
    case 'deepseek':
      return { model, remote_model: 'deepseek-chat', temperature: null, max_tokens: null }
    case 'openai_compatible':
      return {
        model,
        base_url: 'http://localhost:11434/v1',
        remote_model: 'model',
        temperature: null,
        max_tokens: null,
      }
    case 'deepl':
      return { model, base_url: null }
    case 'google_cloud_translation':
      return { model }
    case 'caiyun':
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
