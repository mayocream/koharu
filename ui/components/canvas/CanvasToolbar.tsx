'use client'

import { useEffect, useMemo, useState, type PointerEvent } from 'react'
import * as SelectPrimitive from '@radix-ui/react-select'
import { useQueryClient } from '@tanstack/react-query'
import { useTranslation } from 'react-i18next'

import { motion } from 'motion/react'
import {
  CheckCircleIcon,
  DownloadIcon,
  ScanIcon,
  ScanTextIcon,
  Wand2Icon,
  TypeIcon,
  LoaderCircleIcon,
  LanguagesIcon,
  Trash2Icon,
} from 'lucide-react'
import { Separator } from '@/components/ui/separator'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Textarea } from '@/components/ui/textarea'
import {
  AlertDialog,
  AlertDialogAction,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogTitle,
} from '@/components/ui/alert-dialog'
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
import {
  deleteLocalLlmModel,
  getGetLlmCatalogQueryKey,
  getGetLlmQueryKey,
  useGetLlm,
  useGetLlmCatalog,
} from '@/lib/api/llm/llm'
import type {
  LlmCatalog,
  LlmCatalogModel,
  LlmProviderCatalog,
} from '@/lib/api/schemas'
import { useProcessing } from '@/lib/machines'
import { llmTargetKey, sameLlmTarget } from '@/lib/llmTargets'

type SelectableLlmModel = {
  model: LlmCatalogModel
  provider?: LlmProviderCatalog
}

const selectablePriority = ({ model, provider }: SelectableLlmModel) => {
  if (!provider && model.downloaded) return 0
  if (provider) return 1
  return 2
}

const sortCatalogModels = (
  models: SelectableLlmModel[],
): SelectableLlmModel[] =>
  models
    .map((entry, index) => ({ entry, index }))
    .sort((left, right) => {
      const priority =
        selectablePriority(left.entry) - selectablePriority(right.entry)
      if (priority !== 0) return priority
      return left.index - right.index
    })
    .map(({ entry }) => entry)

const flattenCatalogModels = (catalog?: LlmCatalog): SelectableLlmModel[] =>
  sortCatalogModels([
    ...(catalog?.localModels ?? []).map((model) => ({ model })),
    ...(catalog?.providers ?? [])
      .filter((provider) => provider.status === 'ready')
      .flatMap((provider) =>
        provider.models.map((model) => ({ model, provider })),
      ),
  ])

const filterCatalogModels = (
  models: SelectableLlmModel[],
  query: string,
): SelectableLlmModel[] => {
  const normalized = query.trim().toLowerCase()
  if (!normalized) return models

  return models.filter(({ model, provider }) => {
    const candidates = [
      model.name,
      model.target.modelId,
      model.target.providerId,
      provider?.name,
      provider?.id,
    ]
    return candidates.some((candidate) =>
      candidate?.toLowerCase().includes(normalized),
    )
  })
}

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
  const { data: llmState } = useGetLlm()
  const llmReady = llmState?.status === 'ready'
  const hasDocument = useEditorUiStore(
    (state) => state.currentDocumentId !== null,
  )
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
        disabled={!hasDocument || isDetecting || isProcessing}
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
        disabled={!hasDocument || isOcr || isProcessing}
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
          const selectedLanguage = useEditorUiStore.getState().selectedLanguage
          const { customSystemPrompt } = usePreferencesStore.getState()
          send({
            type: 'START_TRANSLATE',
            documentId,
            options: {
              language: selectedLanguage,
              systemPrompt: customSystemPrompt,
            },
          })
        }}
        disabled={!hasDocument || !llmReady || isTranslating || isProcessing}
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
        disabled={!hasDocument || isInpainting || isProcessing}
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
        disabled={!hasDocument || isRendering || isProcessing}
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
  const queryClient = useQueryClient()
  const [popoverOpen, setPopoverOpen] = useState(false)
  const [deleteDialogOpen, setDeleteDialogOpen] = useState(false)
  const [deleteCandidate, setDeleteCandidate] =
    useState<SelectableLlmModel | null>(null)
  const [deletingModel, setDeletingModel] = useState(false)
  const [modelSearchQuery, setModelSearchQuery] = useState('')
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
  const llmSelectedLanguage = useEditorUiStore(
    (state) => state.selectedLanguage,
  )
  const { data: llmState } = useGetLlm()
  const llmReady = llmState?.status === 'ready'
  const llmStateLoading = llmState?.status === 'loading'
  const { send, state, isProcessing } = useProcessing()
  const llmLoading = state.matches('loadingLlm')
  const llmUnloading = state.matches('unloadingLlm')
  const busy = llmLoading || llmUnloading || llmStateLoading
  const controlsDisabled = busy || deletingModel
  const { t } = useTranslation()

  const selectedModel = useMemo(
    () =>
      llmModels.find(({ model }) =>
        sameLlmTarget(model.target, selectedTarget),
      ),
    [llmModels, selectedTarget],
  )
  const filteredLlmModels = useMemo(
    () => filterCatalogModels(llmModels, modelSearchQuery),
    [llmModels, modelSearchQuery],
  )
  const selectedTargetKey = selectedTarget
    ? llmTargetKey(selectedTarget)
    : undefined
  const selectedModelLanguages = selectedModel?.model.languages ?? []
  const selectedIsLoaded =
    llmReady && sameLlmTarget(llmState?.target, selectedTarget)

  const applySelectedModel = (nextSelection: SelectableLlmModel) => {
    const nextLanguages = nextSelection.model.languages
    const nextLanguage =
      llmSelectedLanguage && nextLanguages.includes(llmSelectedLanguage)
        ? llmSelectedLanguage
        : nextLanguages[0]

    useEditorUiStore.setState({
      selectedTarget: nextSelection.model.target,
      selectedLanguage: nextLanguage,
    })
  }

  const handleSetSelectedModel = (key: string) => {
    const nextSelection = llmModels.find(
      ({ model }) => llmTargetKey(model.target) === key,
    )
    if (!nextSelection) return

    applySelectedModel(nextSelection)
    setModelSearchQuery('')
  }

  const handleSetSelectedLanguage = (language: string) => {
    if (!selectedModelLanguages.includes(language)) return
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

  const handleDeleteModel = async () => {
    const target = deleteCandidate?.model.target
    if (!target || target.kind !== 'local') return

    setDeletingModel(true)
    try {
      await deleteLocalLlmModel(target.modelId)
      await Promise.all([
        queryClient.invalidateQueries({ queryKey: getGetLlmQueryKey() }),
        queryClient.invalidateQueries({
          queryKey: getGetLlmCatalogQueryKey(),
        }),
      ])
      setDeleteDialogOpen(false)
      setDeleteCandidate(null)
    } catch (error) {
      useEditorUiStore
        .getState()
        .showError(
          (error as Error)?.message ??
            t('llm.deleteModelFailed', {
              defaultValue: 'Failed to delete downloaded model',
            }),
        )
    } finally {
      setDeletingModel(false)
    }
  }

  const handleRequestDeleteModel = (modelEntry: SelectableLlmModel) => {
    if (controlsDisabled || isProcessing) return
    setDeleteCandidate(modelEntry)
    setDeleteDialogOpen(true)
  }

  const handleDeleteButtonPointerDown = (
    event: PointerEvent<HTMLButtonElement>,
  ) => {
    event.preventDefault()
    event.stopPropagation()
  }

  const handleDeleteButtonPointerUp = (
    event: PointerEvent<HTMLButtonElement>,
    modelEntry: SelectableLlmModel,
  ) => {
    event.preventDefault()
    event.stopPropagation()
    handleRequestDeleteModel(modelEntry)
  }

  const handleDownloadButtonPointerUp = (
    event: PointerEvent<HTMLButtonElement>,
    modelEntry: SelectableLlmModel,
  ) => {
    event.preventDefault()
    event.stopPropagation()
    if (controlsDisabled || isProcessing || modelEntry.provider) return
    applySelectedModel(modelEntry)
    send({
      type: 'START_LLM_LOAD',
      request: {
        target: modelEntry.model.target,
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

    const nextLanguages = nextModel.languages
    const nextLanguage =
      llmSelectedLanguage && nextLanguages.includes(llmSelectedLanguage)
        ? llmSelectedLanguage
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
  }, [llmModels, llmSelectedLanguage, selectedModel?.model, selectedTarget])

  return (
    <>
      <Popover
        open={popoverOpen}
        onOpenChange={(nextOpen) => {
          setPopoverOpen(nextOpen)
          if (!nextOpen) setModelSearchQuery('')
        }}
      >
        <PopoverTrigger asChild>
          <button
            data-testid='llm-trigger'
            data-llm-ready={llmReady ? 'true' : 'false'}
            data-llm-loading={busy ? 'true' : 'false'}
            className={`flex h-6 cursor-pointer items-center gap-1.5 rounded-full px-2.5 text-[11px] font-medium shadow-sm transition hover:opacity-80 ${
              llmReady
                ? 'bg-rose-400 text-white ring-1 ring-rose-400/30'
                : busy
                  ? 'bg-amber-400 text-white ring-1 ring-amber-400/30'
                  : 'bg-muted text-muted-foreground ring-border/50 ring-1'
            }`}
          >
            <motion.span
              className={`size-1.5 rounded-full ${
                llmReady
                  ? 'bg-white'
                  : busy
                    ? 'bg-white'
                    : 'bg-muted-foreground/40'
              }`}
              animate={
                llmReady
                  ? { opacity: [1, 0.5, 1] }
                  : busy
                    ? { opacity: [1, 0.4, 1] }
                    : { opacity: 1 }
              }
              transition={
                llmReady || busy
                  ? {
                      duration: busy ? 1 : 2,
                      repeat: Infinity,
                      ease: 'easeInOut',
                    }
                  : {}
              }
            />
            LLM
          </button>
        </PopoverTrigger>
        <PopoverContent
          align='end'
          className='w-[304px] p-0'
          data-testid='llm-popover'
        >
          <div className='flex flex-col gap-1 px-3 pt-3 pb-2.5'>
            <span className='text-muted-foreground text-[10px] font-medium uppercase'>
              {t('llm.model', { defaultValue: 'Model' })}
            </span>
            <Input
              data-testid='llm-model-search'
              value={modelSearchQuery}
              onChange={(event) => setModelSearchQuery(event.target.value)}
              placeholder={t('llm.modelSearchPlaceholder', {
                defaultValue: 'Search models',
              })}
              className='h-7 text-xs'
            />
            <div className='flex items-center gap-1.5'>
              <Select
                value={selectedTargetKey}
                onValueChange={handleSetSelectedModel}
              >
                <SelectTrigger
                  data-testid='llm-model-select'
                  className='min-w-0 flex-1'
                >
                  <SelectValue
                    placeholder={t('llm.selectPlaceholder')}
                    className='min-w-0 flex-1'
                  />
                  {selectedModel?.provider ? (
                    <span className='ml-auto flex shrink-0 items-center gap-1.5'>
                      <span className='bg-primary/10 text-primary rounded px-1 py-0.5 text-[9px] leading-none font-semibold uppercase'>
                        {selectedModel.provider.name}
                      </span>
                    </span>
                  ) : null}
                </SelectTrigger>
                <SelectContent position='popper' className='min-w-[19rem]'>
                  {filteredLlmModels.length > 0 ? (
                    filteredLlmModels.map(({ model, provider }, index) => (
                      <SelectPrimitive.Item
                        key={llmTargetKey(model.target)}
                        value={llmTargetKey(model.target)}
                        data-testid={`llm-model-option-${index}`}
                        className='group/item focus:bg-accent focus:text-accent-foreground relative grid w-full cursor-default grid-cols-[minmax(0,1fr)_auto] items-center gap-2 rounded-sm py-1 pr-1.5 pl-1.5 text-xs outline-hidden select-none data-[disabled]:pointer-events-none data-[disabled]:opacity-50'
                      >
                        <SelectPrimitive.ItemText>
                          <span className='truncate'>{model.name}</span>
                        </SelectPrimitive.ItemText>
                        <span className='ml-auto flex shrink-0 items-center gap-1.5'>
                          {provider ? (
                            <span className='bg-primary/10 text-primary rounded px-1 py-0.5 text-[9px] leading-none font-semibold uppercase'>
                              {provider.name}
                            </span>
                          ) : null}
                          {!model.downloaded && !provider ? (
                            <button
                              type='button'
                              data-testid={`llm-model-download-${index}`}
                              onPointerDown={handleDeleteButtonPointerDown}
                              onPointerUp={(event) =>
                                handleDownloadButtonPointerUp(event, {
                                  model,
                                  provider,
                                })
                              }
                              disabled={controlsDisabled || isProcessing}
                              aria-label={t('llm.downloadModelAction', {
                                defaultValue: 'Download',
                              })}
                              className='text-muted-foreground hover:bg-primary/10 hover:text-primary pointer-events-none inline-flex size-5 items-center justify-center rounded-sm opacity-0 transition group-hover/item:pointer-events-auto group-hover/item:opacity-100 disabled:pointer-events-none'
                            >
                              <DownloadIcon className='size-3.5' />
                            </button>
                          ) : null}
                          {model.downloaded && !provider ? (
                            <button
                              type='button'
                              data-testid={`llm-model-delete-${index}`}
                              onPointerDown={handleDeleteButtonPointerDown}
                              onPointerUp={(event) =>
                                handleDeleteButtonPointerUp(event, {
                                  model,
                                  provider,
                                })
                              }
                              disabled={controlsDisabled || isProcessing}
                              aria-label={t('llm.deleteModelAction', {
                                defaultValue: 'Delete',
                              })}
                              className='text-destructive hover:bg-destructive/10 hover:text-destructive pointer-events-none inline-flex size-5 items-center justify-center rounded-sm opacity-0 transition group-hover/item:pointer-events-auto group-hover/item:opacity-100 disabled:pointer-events-none'
                            >
                              <Trash2Icon className='size-3.5' />
                            </button>
                          ) : null}
                          {model.downloaded && !provider ? (
                            <span
                              title={t('llm.downloadedBadge', {
                                defaultValue: 'Downloaded',
                              })}
                              aria-label={t('llm.downloadedBadge', {
                                defaultValue: 'Downloaded',
                              })}
                              className='flex size-4 items-center justify-center rounded bg-emerald-500/12 text-emerald-600'
                            >
                              <CheckCircleIcon className='size-3.5' />
                            </span>
                          ) : null}
                        </span>
                      </SelectPrimitive.Item>
                    ))
                  ) : (
                    <div
                      data-testid='llm-model-empty'
                      className='text-muted-foreground px-2 py-2 text-xs'
                    >
                      {t('llm.modelSearchNoResults', {
                        defaultValue: 'No models found',
                      })}
                    </div>
                  )}
                </SelectContent>
              </Select>
              <Button
                data-testid='llm-load-toggle'
                data-llm-ready={selectedIsLoaded ? 'true' : 'false'}
                data-llm-loading={busy ? 'true' : 'false'}
                variant={selectedIsLoaded ? 'ghost' : 'default'}
                size='sm'
                onClick={handleToggleLoadUnload}
                disabled={!selectedTarget || controlsDisabled}
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

          <div className='flex flex-col gap-1 px-3 pt-2.5 pb-3'>
            <span className='text-muted-foreground text-[10px] font-medium uppercase'>
              {t('llm.translationSettings', {
                defaultValue: 'Translation',
              })}
            </span>

            <div className='flex flex-col gap-1.5'>
              {selectedModelLanguages.length > 0 ? (
                <Select
                  value={llmSelectedLanguage ?? selectedModelLanguages[0]}
                  onValueChange={handleSetSelectedLanguage}
                >
                  <SelectTrigger
                    data-testid='llm-language-select'
                    className='w-full'
                  >
                    <SelectValue placeholder={t('llm.languagePlaceholder')} />
                  </SelectTrigger>
                  <SelectContent position='popper'>
                    {selectedModelLanguages.map((language, index) => (
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
            </div>
          </div>
        </PopoverContent>
      </Popover>

      <AlertDialog
        open={deleteDialogOpen}
        onOpenChange={(open) => {
          setDeleteDialogOpen(open)
          if (!open && !deletingModel) {
            setDeleteCandidate(null)
          }
        }}
      >
        <AlertDialogContent>
          <AlertDialogTitle>
            {t('llm.deleteModelTitle', {
              defaultValue: 'Delete downloaded model?',
            })}
          </AlertDialogTitle>
          <AlertDialogDescription>
            {t('llm.deleteModelDescription', {
              defaultValue:
                'This removes the local model file. Loading the model again will download it again.',
            })}
          </AlertDialogDescription>
          <div className='flex justify-end gap-2'>
            <AlertDialogCancel disabled={deletingModel}>
              {t('common.cancel')}
            </AlertDialogCancel>
            <AlertDialogAction
              onClick={() => void handleDeleteModel()}
              disabled={deletingModel}
            >
              {deletingModel
                ? t('llm.deleteModelDeleting', {
                    defaultValue: 'Deleting...',
                  })
                : t('llm.deleteModelAction', {
                    defaultValue: 'Delete',
                  })}
            </AlertDialogAction>
          </div>
        </AlertDialogContent>
      </AlertDialog>
    </>
  )
}
