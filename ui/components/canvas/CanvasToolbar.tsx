'use client'

import { useEffect, useMemo } from 'react'
import { useTranslation } from 'react-i18next'

import { motion } from 'motion/react'
import {
  ScanIcon,
  ScanTextIcon,
  Wand2Icon,
  TypeIcon,
  LoaderCircleIcon,
  LanguagesIcon,
} from 'lucide-react'
import { Separator } from '@/components/ui/separator'
import { Button } from '@/components/ui/button'
import { Textarea } from '@/components/ui/textarea'
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
import { useEditorUiStore } from '@/lib/stores/editorUiStore'
import { usePreferencesStore } from '@/lib/stores/preferencesStore'
import { useGetLlm, useGetLlmCatalog } from '@/lib/api/llm/llm'
import { useGetTranslateReady } from '@/lib/api/processing/processing'
import { useGetConfig } from '@/lib/api/system/system'
import type {
  LlmCatalog,
  LlmCatalogModel,
  LlmProviderCatalog,
} from '@/lib/api/schemas'
import { useProcessing } from '@/lib/machines'
import { llmTargetKey, sameLlmTarget } from '@/lib/llmTargets'
import {
  effectiveTranslationLanguage,
  translationTargetLocalesFromCatalog,
} from '@/lib/translationTargetLocales'
import {
  isPipelineMachineTranslator,
  PIPELINE_TRANSLATOR_DEEPL,
  PIPELINE_TRANSLATOR_GOOGLE,
} from '@/lib/pipelineTranslator'
import {
  DEEPL_SELECT_OMIT,
  deepLTranslateOptionsForRequest,
  deeplSelectValue,
  deeplStoredFromSelect,
} from '@/lib/deeplTranslateRequest'

type SelectableLlmModel = {
  model: LlmCatalogModel
  provider?: LlmProviderCatalog
}

const flattenCatalogModels = (catalog?: LlmCatalog): SelectableLlmModel[] => [
  ...(catalog?.localModels ?? []).map((model) => ({ model })),
  ...(catalog?.providers ?? [])
    .filter((provider) => provider.status === 'ready')
    .flatMap((provider) =>
      provider.models.map((model) => ({ model, provider })),
    ),
]

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
  const { send, isProcessing, state } = useProcessing()
  const { data: translateReadyState } = useGetTranslateReady()
  const { data: llmCatalog } = useGetLlmCatalog()
  const { data: appConfig } = useGetConfig()
  const translateReady = translateReadyState?.ready ?? false
  const { t } = useTranslation()

  const isDetecting = state.matches('detecting')
  const isOcr = state.matches('recognizing')
  const isInpainting = state.matches('inpainting')
  const isRendering = state.matches('rendering')
  const isTranslating = state.matches('translating')

  const requireDocumentId = () => {
    const id = useEditorUiStore.getState().currentDocumentId
    if (!id) throw new Error('No current document selected')
    return id
  }

  return (
    <div className='flex items-center gap-0.5'>
      <Button
        variant='ghost'
        size='xs'
        onClick={() =>
          send({ type: 'START_DETECT', documentId: requireDocumentId() })
        }
        data-testid='toolbar-detect'
        disabled={isDetecting || isProcessing}
      >
        {isDetecting ? (
          <LoaderCircleIcon className='size-4 animate-spin' />
        ) : (
          <ScanIcon className='size-4' />
        )}
        {t('processing.detect')}
      </Button>

      <Separator orientation='vertical' className='mx-0.5 h-4' />

      <Button
        variant='ghost'
        size='xs'
        onClick={() =>
          send({ type: 'START_RECOGNIZE', documentId: requireDocumentId() })
        }
        data-testid='toolbar-ocr'
        disabled={isOcr || isProcessing}
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
        onClick={() => {
          const documentId = requireDocumentId()
          const { selectedLanguage } = useEditorUiStore.getState()
          const prefs = usePreferencesStore.getState()
          const { customSystemPrompt } = prefs
          const deepl = deepLTranslateOptionsForRequest(
            appConfig?.pipeline?.translator,
            prefs,
          )
          send({
            type: 'START_TRANSLATE',
            documentId,
            options: {
              language: effectiveTranslationLanguage(
                llmCatalog,
                selectedLanguage,
              ),
              systemPrompt: customSystemPrompt,
              ...(deepl ? { deepl } : {}),
            },
          })
        }}
        disabled={!translateReady || isTranslating || isProcessing}
        data-testid='toolbar-translate'
      >
        {isTranslating ? (
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
        onClick={() =>
          send({ type: 'START_INPAINT', documentId: requireDocumentId() })
        }
        data-testid='toolbar-inpaint'
        disabled={isInpainting || isProcessing}
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
        onClick={() => {
          const documentId = requireDocumentId()
          const { renderEffect, renderStroke } = useEditorUiStore.getState()
          send({
            type: 'START_RENDER',
            documentId,
            options: {
              shaderEffect: renderEffect,
              shaderStroke: renderStroke,
            },
          })
        }}
        data-testid='toolbar-render'
        disabled={isRendering || isProcessing}
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
  const { data: llmCatalog } = useGetLlmCatalog()
  const { data: appConfig } = useGetConfig()
  const pipelineTranslator = appConfig?.pipeline?.translator
  const machineTranslator = isPipelineMachineTranslator(pipelineTranslator)
  const llmModels = useMemo(
    () => flattenCatalogModels(llmCatalog),
    [llmCatalog],
  )
  const selectedTarget = useEditorUiStore((state) => state.selectedTarget)
  const customSystemPrompt = usePreferencesStore(
    (state) => state.customSystemPrompt,
  )
  const setCustomSystemPrompt = usePreferencesStore(
    (state) => state.setCustomSystemPrompt,
  )
  const deeplFormality = usePreferencesStore((s) => s.deeplFormality)
  const deeplModelType = usePreferencesStore((s) => s.deeplModelType)
  const setDeeplFormality = usePreferencesStore((s) => s.setDeeplFormality)
  const setDeeplModelType = usePreferencesStore((s) => s.setDeeplModelType)
  const llmSelectedLanguage = useEditorUiStore(
    (state) => state.selectedLanguage,
  )
  const { data: llmState } = useGetLlm()
  const { data: translateReadyState } = useGetTranslateReady()
  const llmReady = llmState?.status === 'ready'
  const translateReady = translateReadyState?.ready ?? false
  const triggerReady =
    llmReady || (machineTranslator && translateReady)
  const { send, state } = useProcessing()
  const llmLoading = state.matches('loadingLlm')
  const llmUnloading = state.matches('unloadingLlm')
  const busy = llmLoading || llmUnloading
  const { t } = useTranslation()

  const triggerLabel = useMemo(() => {
    switch (pipelineTranslator) {
      case PIPELINE_TRANSLATOR_GOOGLE:
        return t('llm.translatorTrigger.google', { defaultValue: 'Google' })
      case PIPELINE_TRANSLATOR_DEEPL:
        return t('llm.translatorTrigger.deepl', { defaultValue: 'DeepL' })
      default:
        return t('llm.translatorTrigger.llm', { defaultValue: 'LLM' })
    }
  }, [pipelineTranslator, t])

  const selectedModel = useMemo(
    () =>
      llmModels.find(({ model }) =>
        sameLlmTarget(model.target, selectedTarget),
      ),
    [llmModels, selectedTarget],
  )
  const selectedTargetKey = selectedTarget
    ? llmTargetKey(selectedTarget)
    : undefined
  const selectedModelLanguages = selectedModel?.model.languages ?? []
  const translationLocales = useMemo(() => {
    if (machineTranslator) {
      return translationTargetLocalesFromCatalog(llmCatalog)
    }
    if (selectedModelLanguages.length > 0) return selectedModelLanguages
    return translationTargetLocalesFromCatalog(llmCatalog)
  }, [machineTranslator, llmCatalog, selectedModelLanguages])
  const selectedIsLoaded =
    !machineTranslator &&
    llmReady &&
    sameLlmTarget(llmState?.target, selectedTarget)

  const handleSetSelectedModel = (key: string) => {
    const nextSelection = llmModels.find(
      ({ model }) => llmTargetKey(model.target) === key,
    )
    if (!nextSelection) return

    const nextLanguages =
      nextSelection.model.languages.length > 0
        ? nextSelection.model.languages
        : translationTargetLocalesFromCatalog(llmCatalog)
    const nextLanguage =
      llmSelectedLanguage && nextLanguages.includes(llmSelectedLanguage)
        ? llmSelectedLanguage
        : nextLanguages[0]

    useEditorUiStore.setState({
      selectedTarget: nextSelection.model.target,
      selectedLanguage: nextLanguage,
    })
  }

  const handleSetSelectedLanguage = (language: string) => {
    if (!translationLocales.includes(language)) return
    useEditorUiStore.setState({ selectedLanguage: language })
  }

  const handleToggleLoadUnload = () => {
    const currentSelectedTarget = useEditorUiStore.getState().selectedTarget
    if (!currentSelectedTarget) return

    if (selectedIsLoaded) {
      send({ type: 'START_LLM_UNLOAD' })
      return
    }

    send({
      type: 'START_LLM_LOAD',
      request: {
        target: currentSelectedTarget,
      },
    })
  }

  useEffect(() => {
    if (llmModels.length === 0) return

    const hasCurrent = llmModels.some(({ model }) =>
      sameLlmTarget(model.target, selectedTarget),
    )
    const nextModel = hasCurrent ? selectedModel?.model : llmModels[0]?.model
    if (!nextModel) return

    const catalogLocales = translationTargetLocalesFromCatalog(llmCatalog)
    const nextLanguages = machineTranslator
      ? catalogLocales
      : nextModel.languages.length > 0
        ? nextModel.languages
        : catalogLocales

    const currentLang = useEditorUiStore.getState().selectedLanguage
    const nextLanguage =
      currentLang && nextLanguages.includes(currentLang)
        ? currentLang
        : nextLanguages[0]

    const currentState = useEditorUiStore.getState()
    if (
      sameLlmTarget(currentState.selectedTarget, nextModel.target) &&
      currentState.selectedLanguage === nextLanguage
    ) {
      return
    }

    useEditorUiStore.setState({
      selectedTarget: nextModel.target,
      selectedLanguage: nextLanguage,
    })
  }, [
    machineTranslator,
    llmCatalog,
    llmModels,
    selectedModel?.model,
    selectedTarget,
  ])

  return (
    <Popover>
      <PopoverTrigger asChild>
        <button
          data-testid='llm-trigger'
          data-pipeline-translator={pipelineTranslator ?? 'llm'}
          data-llm-ready={triggerReady ? 'true' : 'false'}
          data-llm-loading={busy ? 'true' : 'false'}
          className={`flex h-6 max-w-[140px] cursor-pointer items-center gap-1.5 rounded-full px-2.5 text-[11px] font-medium shadow-sm transition hover:opacity-80 ${
            triggerReady
              ? 'bg-rose-400 text-white ring-1 ring-rose-400/30'
              : busy
                ? 'bg-amber-400 text-white ring-1 ring-amber-400/30'
                : 'bg-muted text-muted-foreground ring-border/50 ring-1'
          }`}
        >
          <motion.span
            className={`size-1.5 shrink-0 rounded-full ${
              triggerReady
                ? 'bg-white'
                : busy
                  ? 'bg-white'
                  : 'bg-muted-foreground/40'
            }`}
            animate={
              triggerReady
                ? { opacity: [1, 0.5, 1] }
                : busy
                  ? { opacity: [1, 0.4, 1] }
                  : { opacity: 1 }
            }
            transition={
              triggerReady || busy
                ? {
                    duration: busy ? 1 : 2,
                    repeat: Infinity,
                    ease: 'easeInOut',
                  }
                : {}
            }
          />
          <span className='min-w-0 truncate'>{triggerLabel}</span>
        </button>
      </PopoverTrigger>
      <PopoverContent
        align='end'
        className='w-[min(100vw-2rem,300px)] p-0'
        data-testid='llm-popover'
      >
        {!machineTranslator ? (
          <>
            <div className='flex flex-col gap-1 px-3 pt-3 pb-2.5'>
              <span className='text-muted-foreground text-[10px] font-medium uppercase'>
                {t('llm.model', { defaultValue: 'Model' })}
              </span>
              <div className='flex items-center gap-1.5'>
                <Select
                  value={selectedTargetKey}
                  onValueChange={handleSetSelectedModel}
                >
                  <SelectTrigger
                    data-testid='llm-model-select'
                    className='min-w-0 flex-1'
                  >
                    <SelectValue placeholder={t('llm.selectPlaceholder')} />
                  </SelectTrigger>
                  <SelectContent position='popper'>
                    {llmModels.map(({ model, provider }, index) => (
                      <SelectItem
                        key={llmTargetKey(model.target)}
                        value={llmTargetKey(model.target)}
                        data-testid={`llm-model-option-${index}`}
                      >
                        <span className='flex items-center gap-1.5'>
                          {provider ? (
                            <span className='bg-primary/10 text-primary rounded px-1 py-0.5 text-[9px] leading-none font-semibold uppercase'>
                              {provider.name}
                            </span>
                          ) : null}
                          {model.name}
                        </span>
                      </SelectItem>
                    ))}
                  </SelectContent>
                </Select>
                <Button
                  data-testid='llm-load-toggle'
                  data-llm-ready={selectedIsLoaded ? 'true' : 'false'}
                  data-llm-loading={busy ? 'true' : 'false'}
                  variant={selectedIsLoaded ? 'ghost' : 'default'}
                  size='sm'
                  onClick={handleToggleLoadUnload}
                  disabled={!selectedTarget || busy}
                  className='h-6 shrink-0 gap-1 px-2 text-[11px]'
                >
                  {busy ? (
                    <LoaderCircleIcon className='size-3 animate-spin' />
                  ) : null}
                  {selectedIsLoaded || llmUnloading
                    ? t('llm.unload')
                    : t('llm.load')}
                </Button>
              </div>
            </div>

            <div className='px-3'>
              <Separator />
            </div>
          </>
        ) : null}

        {/* Language + prompt */}
        <div
          className={`flex flex-col gap-1 px-3 pb-3 ${machineTranslator ? 'pt-3' : 'pt-2.5'}`}
        >
          <span className='text-muted-foreground text-[10px] font-medium uppercase'>
            {t('llm.translationSettings', {
              defaultValue: 'Translation',
            })}
          </span>

          <div className='flex flex-col gap-1.5'>
            {translationLocales.length > 0 ? (
              <Select
                value={llmSelectedLanguage ?? translationLocales[0]}
                onValueChange={handleSetSelectedLanguage}
              >
                <SelectTrigger
                  data-testid='llm-language-select'
                  className='w-full'
                >
                  <SelectValue placeholder={t('llm.languagePlaceholder')} />
                </SelectTrigger>
                <SelectContent position='popper'>
                  {translationLocales.map((language, index) => (
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
            ) : null}

            {!machineTranslator ? (
              <Textarea
                data-testid='llm-system-prompt'
                value={customSystemPrompt ?? ''}
                onChange={(e) =>
                  setCustomSystemPrompt(e.target.value || undefined)
                }
                placeholder={t('llm.systemPromptPlaceholder', {
                  defaultValue: 'Custom system prompt (optional)',
                })}
                rows={5}
                className='min-h-0 resize-y text-xs'
              />
            ) : pipelineTranslator === PIPELINE_TRANSLATOR_DEEPL ? (
              <div className='flex flex-col gap-2'>
                <span className='text-muted-foreground text-[10px] font-medium uppercase'>
                  {t('llm.deeplOptions', { defaultValue: 'DeepL options' })}
                </span>
                <div className='flex flex-col gap-1'>
                  <span className='text-muted-foreground text-[9px]'>
                    {t('llm.deeplFormality', { defaultValue: 'Formality' })}
                  </span>
                  <Select
                    value={deeplSelectValue(deeplFormality)}
                    onValueChange={(v) =>
                      setDeeplFormality(deeplStoredFromSelect(v))
                    }
                  >
                    <SelectTrigger
                      data-testid='deepl-formality-select'
                      className='w-full'
                    >
                      <SelectValue />
                    </SelectTrigger>
                    <SelectContent position='popper'>
                      <SelectItem value={DEEPL_SELECT_OMIT}>
                        {t('llm.deeplFormalityDefault', {
                          defaultValue: 'Default (API)',
                        })}
                      </SelectItem>
                      <SelectItem value='default'>default</SelectItem>
                      <SelectItem value='more'>more</SelectItem>
                      <SelectItem value='less'>less</SelectItem>
                      <SelectItem value='prefer_more'>prefer_more</SelectItem>
                      <SelectItem value='prefer_less'>prefer_less</SelectItem>
                    </SelectContent>
                  </Select>
                </div>
                <div className='flex flex-col gap-1'>
                  <span className='text-muted-foreground text-[9px]'>
                    {t('llm.deeplModelType', { defaultValue: 'Model type' })}
                  </span>
                  <Select
                    value={deeplSelectValue(deeplModelType)}
                    onValueChange={(v) =>
                      setDeeplModelType(deeplStoredFromSelect(v))
                    }
                  >
                    <SelectTrigger
                      data-testid='deepl-model-type-select'
                      className='w-full'
                    >
                      <SelectValue />
                    </SelectTrigger>
                    <SelectContent position='popper'>
                      <SelectItem value={DEEPL_SELECT_OMIT}>
                        {t('llm.deeplModelDefault', {
                          defaultValue: 'Default (API)',
                        })}
                      </SelectItem>
                      <SelectItem value='quality_optimized'>
                        quality_optimized
                      </SelectItem>
                      <SelectItem value='prefer_quality_optimized'>
                        prefer_quality_optimized
                      </SelectItem>
                      <SelectItem value='latency_optimized'>
                        latency_optimized
                      </SelectItem>
                    </SelectContent>
                  </Select>
                </div>
                <p className='text-muted-foreground text-[10px] leading-snug'>
                  {t('llm.deeplOptionsHint', {
                    defaultValue:
                      'Formality applies only to some target languages. DeepL returns an error if unsupported.',
                  })}
                </p>
              </div>
            ) : (
              <p className='text-muted-foreground text-[10px] leading-snug'>
                {t('llm.machineTranslatorHint', {
                  defaultValue:
                    'API translation ignores the local model. Change translator in Settings → Engines.',
                })}
              </p>
            )}
          </div>
        </div>
      </PopoverContent>
    </Popover>
  )
}
