'use client'

import { useEffect, useMemo, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { motion } from 'motion/react'
import {
  ScanIcon,
  ScanTextIcon,
  Wand2Icon,
  TypeIcon,
  LoaderCircleIcon,
  LanguagesIcon,
  SearchIcon,
} from 'lucide-react'
import { Separator } from '@/components/ui/separator'
import { Button } from '@/components/ui/button'
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select'
import {
  Popover,
  PopoverContent,
  PopoverTrigger,
} from '@/components/ui/popover'
import { useLlmUiStore } from '@/lib/stores/llmUiStore'
import {
  useLlmModelsQuery,
  useLlmReadyQuery,
  LOCAL_LLM_PRESET_LABELS,
  parsePresetFromModelId,
} from '@/lib/query/hooks'
import { useDocumentMutations, useLlmMutations } from '@/lib/query/mutations'
import { useOperationStore } from '@/lib/stores/operationStore'
import { usePreferencesStore } from '@/lib/stores/preferencesStore'
import { getProviderDisplayName } from '@/lib/providers'

export function CanvasToolbar() {
  return (
    <div className='border-border/60 bg-card text-foreground flex items-center gap-2 border-b px-3 py-2 text-xs'>
      <WorkflowButtons />
      <div className='flex-1' />
      <LlmStatusPopover />
    </div>
  )
}

function WorkflowButtons() {
  const { inpaint, detect, detectSensitive, ocr, render } =
    useDocumentMutations()
  const { llmGenerate } = useLlmMutations()
  const { data: llmReady = false } = useLlmReadyQuery()
  const [generating, setGenerating] = useState(false)
  const [detectVariant, setDetectVariant] = useState<
    'normal' | 'sensitive' | null
  >(null)
  const { t } = useTranslation()
  const operation = useOperationStore((state) => state.operation)

  const isDetecting =
    operation?.type === 'process-current' && operation?.step === 'detect'
  const isOcr =
    operation?.type === 'process-current' && operation?.step === 'ocr'
  const isInpainting =
    operation?.type === 'process-current' && operation?.step === 'inpaint'
  const isRendering =
    operation?.type === 'process-current' && operation?.step === 'render'

  // Clear variant tracking when detection finishes
  useEffect(() => {
    if (!isDetecting) setDetectVariant(null)
  }, [isDetecting])

  const handleDetect = () => {
    setDetectVariant('normal')
    void detect()
  }

  const handleDetectSensitive = () => {
    setDetectVariant('sensitive')
    void detectSensitive()
  }

  const handleTranslate = async () => {
    setGenerating(true)
    try {
      await llmGenerate(null)
    } catch (error) {
      console.error(error)
    } finally {
      setGenerating(false)
    }
  }

  return (
    <div className='flex items-center gap-0.5'>
      <Button
        variant='ghost'
        size='xs'
        onClick={handleDetect}
        data-testid='toolbar-detect'
        disabled={isDetecting}
      >
        {isDetecting && detectVariant === 'normal' ? (
          <LoaderCircleIcon className='size-4 animate-spin' />
        ) : (
          <ScanIcon className='size-4' />
        )}
        {t('processing.detect')}
      </Button>

      <Button
        variant='ghost'
        size='xs'
        onClick={handleDetectSensitive}
        data-testid='toolbar-detect-sensitive'
        disabled={isDetecting}
        title='Sensitive detection — lower thresholds, catches more text (SFX, small text, complex backgrounds)'
      >
        {isDetecting && detectVariant === 'sensitive' ? (
          <LoaderCircleIcon className='size-4 animate-spin' />
        ) : (
          <SearchIcon className='size-4' />
        )}
        {t('processing.detectSensitive', { defaultValue: 'Detect+' })}
      </Button>

      <Separator orientation='vertical' className='mx-0.5 h-4' />

      <Button
        variant='ghost'
        size='xs'
        onClick={ocr}
        data-testid='toolbar-ocr'
        disabled={isOcr}
      >
        {isOcr ? (
          <LoaderCircleIcon className='size-4 animate-spin' />
        ) : (
          <ScanTextIcon className='size-4' />
        )}
        {t('processing.ocr')}
      </Button>

      <Separator orientation='vertical' className='mx-0.5 h-4' />

      <Button
        variant='ghost'
        size='xs'
        onClick={handleTranslate}
        disabled={!llmReady || generating}
        data-testid='toolbar-translate'
      >
        {generating ? (
          <LoaderCircleIcon className='size-4 animate-spin' />
        ) : (
          <LanguagesIcon className='size-4' />
        )}
        {t('llm.generate')}
      </Button>

      <Separator orientation='vertical' className='mx-0.5 h-4' />

      <Button
        variant='ghost'
        size='xs'
        onClick={inpaint}
        data-testid='toolbar-inpaint'
        disabled={isInpainting}
      >
        {isInpainting ? (
          <LoaderCircleIcon className='size-4 animate-spin' />
        ) : (
          <Wand2Icon className='size-4' />
        )}
        {t('mask.inpaint')}
      </Button>

      <Separator orientation='vertical' className='mx-0.5 h-4' />

      <Button
        variant='ghost'
        size='xs'
        onClick={render}
        data-testid='toolbar-render'
        disabled={isRendering}
      >
        {isRendering ? (
          <LoaderCircleIcon className='size-4 animate-spin' />
        ) : (
          <TypeIcon className='size-4' />
        )}
        {t('llm.render')}
      </Button>
    </div>
  )
}

function LlmStatusPopover() {
  const { data: llmModels = [] } = useLlmModelsQuery()
  const llmSelectedModel = useLlmUiStore((state) => state.selectedModel)
  const llmSelectedLanguage = useLlmUiStore((state) => state.selectedLanguage)
  const llmLoading = useLlmUiStore((state) => state.loading)
  const { data: llmReady = false } = useLlmReadyQuery()
  const { llmSetSelectedModel, llmSetSelectedLanguage, llmToggleLoadUnload } =
    useLlmMutations()
  const { t } = useTranslation()
  const apiKeys = usePreferencesStore((state) => state.apiKeys)
  const localLlm = usePreferencesStore((state) => state.localLlm)

  const selectedModelInfo = useMemo(
    () => llmModels.find((m) => m.id === llmSelectedModel),
    [llmModels, llmSelectedModel],
  )
  const isApiModel =
    selectedModelInfo?.source !== 'local' &&
    selectedModelInfo?.source !== undefined
  const apiKeyMissing =
    isApiModel &&
    selectedModelInfo?.source !== 'openai-compatible' &&
    !apiKeys[selectedModelInfo!.source]

  const activeLanguages = useMemo(
    () => selectedModelInfo?.languages ?? [],
    [selectedModelInfo],
  )

  useEffect(() => {
    if (llmModels.length === 0) return
    const hasCurrent = llmModels.some((model) => model.id === llmSelectedModel)
    const nextModel = hasCurrent ? llmSelectedModel : llmModels[0]?.id
    if (!nextModel) return
    const languages =
      llmModels.find((model) => model.id === nextModel)?.languages ?? []
    const nextLanguage =
      llmSelectedLanguage && languages.includes(llmSelectedLanguage)
        ? llmSelectedLanguage
        : languages[0]
    const currentState = useLlmUiStore.getState()
    if (
      currentState.selectedModel === nextModel &&
      currentState.selectedLanguage === nextLanguage
    ) {
      return
    }
    useLlmUiStore.setState((state) => ({
      selectedModel: nextModel,
      selectedLanguage: nextLanguage,
      loading: state.loading,
    }))
  }, [llmModels, llmSelectedLanguage, llmSelectedModel])

  return (
    <Popover>
      <PopoverTrigger asChild>
        <button
          data-testid='llm-trigger'
          data-llm-ready={llmReady ? 'true' : 'false'}
          data-llm-loading={llmLoading ? 'true' : 'false'}
          className={`flex h-6 cursor-pointer items-center gap-1.5 rounded-full px-2.5 text-[11px] font-medium shadow-sm transition hover:opacity-80 ${
            llmReady
              ? 'bg-rose-400 text-white ring-1 ring-rose-400/30'
              : 'bg-muted text-muted-foreground ring-border/50 ring-1'
          }`}
        >
          <motion.span
            className={`size-1.5 rounded-full ${
              llmReady ? 'bg-white' : 'bg-muted-foreground/40'
            }`}
            animate={llmReady ? { opacity: [1, 0.5, 1] } : { opacity: 1 }}
            transition={
              llmReady
                ? { duration: 2, repeat: Infinity, ease: 'easeInOut' }
                : {}
            }
          />
          LLM
          {llmReady && selectedModelInfo?.source === 'openai-compatible' && (
            <span className='max-w-[80px] truncate text-[10px] opacity-80'>
              {selectedModelInfo.id.split(':')[1] ?? selectedModelInfo.id}
            </span>
          )}
        </button>
      </PopoverTrigger>
      <PopoverContent align='end' className='w-72' data-testid='llm-popover'>
        <div className='space-y-3 text-sm'>
          <p className='text-muted-foreground text-xs font-medium uppercase'>
            {t('panels.llm')}
          </p>

          <Select value={llmSelectedModel} onValueChange={llmSetSelectedModel}>
            <SelectTrigger data-testid='llm-model-select' className='w-full'>
              <SelectValue placeholder={t('llm.selectPlaceholder')} />
            </SelectTrigger>
            <SelectContent position='popper'>
              {llmModels.map((model, index) => (
                <SelectItem
                  key={model.id}
                  value={model.id}
                  data-testid={`llm-model-option-${index}`}
                >
                  <span className='flex items-center gap-2'>
                    {model.source === 'openai-compatible' ? (
                      (() => {
                        const preset = parsePresetFromModelId(model.id)
                        const isTeal =
                          preset === 'preset1' || preset === 'preset2'
                        return (
                          <span
                            className={`rounded px-1 py-0.5 text-[10px] leading-none font-semibold uppercase ${
                              isTeal
                                ? 'bg-teal-500/10 text-teal-600 dark:text-teal-400'
                                : 'bg-emerald-500/10 text-emerald-600 dark:text-emerald-400'
                            }`}
                          >
                            {preset
                              ? (LOCAL_LLM_PRESET_LABELS[preset] ?? preset)
                              : 'OpenAI-like'}
                          </span>
                        )
                      })()
                    ) : model.source !== 'local' ? (
                      <span className='bg-primary/10 text-primary rounded px-1 py-0.5 text-[10px] leading-none font-semibold uppercase'>
                        {getProviderDisplayName(model.source)}
                      </span>
                    ) : null}
                    {/* Display model name: strip "openai-compatible:preset:" prefix */}
                    {model.source === 'openai-compatible' &&
                    model.id.split(':').length >= 3
                      ? model.id.split(':').slice(2).join(':')
                      : model.id.includes(':')
                        ? model.id.split(':')[1]
                        : model.id}
                  </span>
                </SelectItem>
              ))}
            </SelectContent>
          </Select>

          {/* API key warning */}
          {apiKeyMissing && (
            <p className='text-xs text-amber-500'>
              {t('llm.apiKeyMissing', {
                provider: getProviderDisplayName(selectedModelInfo!.source),
              })}
            </p>
          )}

          {/* Loaded model info card */}
          {llmReady &&
            selectedModelInfo?.source === 'openai-compatible' &&
            (() => {
              const infoPreset = parsePresetFromModelId(selectedModelInfo.id)
              const isTeal =
                infoPreset === 'preset1' || infoPreset === 'preset2'
              const presetLabel = infoPreset
                ? (LOCAL_LLM_PRESET_LABELS[infoPreset] ?? infoPreset)
                : 'OpenAI-like'
              const presetCfg = infoPreset
                ? localLlm.presets[infoPreset]
                : undefined
              const modelName =
                selectedModelInfo.id.split(':').length >= 3
                  ? selectedModelInfo.id.split(':').slice(2).join(':')
                  : (selectedModelInfo.id.split(':')[1] ?? selectedModelInfo.id)
              return (
                <div
                  className={`rounded-md px-2.5 py-2 text-xs ${
                    isTeal
                      ? 'border border-teal-500/20 bg-teal-500/5'
                      : 'border border-emerald-500/20 bg-emerald-500/5'
                  }`}
                >
                  <div className='flex items-center gap-1.5'>
                    <span
                      className={`size-1.5 rounded-full ${
                        isTeal ? 'bg-teal-500' : 'bg-emerald-500'
                      }`}
                    />
                    <span
                      className={`font-medium ${
                        isTeal
                          ? 'text-teal-700 dark:text-teal-400'
                          : 'text-emerald-700 dark:text-emerald-400'
                      }`}
                    >
                      {t('llm.localModelActive')}
                    </span>
                  </div>
                  <p className='text-muted-foreground mt-1'>
                    {t('llm.localModelName', { name: modelName })}
                  </p>
                  <p className='text-muted-foreground mt-0.5'>
                    {presetLabel}
                    {presetCfg?.temperature != null &&
                      ` · temp ${presetCfg.temperature}`}
                    {presetCfg?.maxTokens != null &&
                      ` · ${presetCfg.maxTokens} tokens`}
                  </p>
                </div>
              )
            })()}

          {activeLanguages.length > 0 && (
            <Select
              value={llmSelectedLanguage ?? activeLanguages[0]}
              onValueChange={llmSetSelectedLanguage}
            >
              <SelectTrigger
                data-testid='llm-language-select'
                className='w-full'
              >
                <SelectValue placeholder={t('llm.languagePlaceholder')} />
              </SelectTrigger>
              <SelectContent position='popper'>
                {activeLanguages.map((language, index) => (
                  <SelectItem
                    key={language}
                    value={language}
                    data-testid={`llm-language-option-${index}`}
                  >
                    {t(`llm.languages.${language}`)}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
          )}

          <Button
            data-testid='llm-load-toggle'
            data-llm-ready={llmReady ? 'true' : 'false'}
            data-llm-loading={llmLoading ? 'true' : 'false'}
            variant='outline'
            size='sm'
            onClick={llmToggleLoadUnload}
            disabled={!llmSelectedModel || llmLoading}
            className='w-full gap-1.5 text-xs'
          >
            {llmLoading && (
              <LoaderCircleIcon className='size-3.5 animate-spin' />
            )}
            {!llmReady ? t('llm.load') : t('llm.unload')}
          </Button>
        </div>
      </PopoverContent>
    </Popover>
  )
}
