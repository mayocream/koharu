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
} from 'lucide-react'
import { Separator } from '@/components/ui/separator'
import { useTextBlocks } from '@/hooks/useTextBlocks'
import { RenderEffect, RgbaColor, TextStyle } from '@/types'
import { Button } from '@/components/ui/button'
import { ColorPicker } from '@/components/ui/color-picker'
import {
  Tooltip,
  TooltipContent,
  TooltipTrigger,
} from '@/components/ui/tooltip'
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
import { useLlmUiStore } from '@/lib/stores/llmUiStore'
import {
  useFontsQuery,
  useLlmModelsQuery,
  useLlmReadyQuery,
} from '@/lib/query/hooks'
import {
  useDocumentMutations,
  useLlmMutations,
  useTextBlockMutations,
} from '@/lib/query/mutations'

const DEFAULT_COLOR: RgbaColor = [0, 0, 0, 255]
const DEFAULT_FONT_FAMILIES = ['Arial']

const clampByte = (value: number) =>
  Math.max(0, Math.min(255, Math.round(value)))

const colorToHex = (color: RgbaColor) =>
  `#${color
    .slice(0, 3)
    .map((value) => value.toString(16).padStart(2, '0'))
    .join('')}`

const hexToColor = (value: string, alpha: number): RgbaColor => {
  const normalized = value.replace('#', '')
  if (normalized.length !== 6) {
    return [0, 0, 0, clampByte(alpha)]
  }

  const r = Number.parseInt(normalized.slice(0, 2), 16)
  const g = Number.parseInt(normalized.slice(2, 4), 16)
  const b = Number.parseInt(normalized.slice(4, 6), 16)

  if ([r, g, b].some((channel) => Number.isNaN(channel))) {
    return [0, 0, 0, clampByte(alpha)]
  }

  return [r, g, b, clampByte(alpha)]
}

const uniqueStrings = (values: string[]) => {
  const seen = new Set<string>()
  return values.filter((value) => {
    if (!value || seen.has(value)) return false
    seen.add(value)
    return true
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
  const { inpaint, detect, ocr, render } = useDocumentMutations()
  const { llmGenerate } = useLlmMutations()
  const { data: llmReady = false } = useLlmReadyQuery()
  const [generating, setGenerating] = useState(false)
  const { t } = useTranslation()

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
        onClick={detect}
        data-testid='toolbar-detect'
      >
        <ScanIcon className='size-4' />
        {t('processing.detect')}
      </Button>

      <Separator orientation='vertical' className='mx-0.5 h-4' />

      <Button variant='ghost' size='xs' onClick={ocr} data-testid='toolbar-ocr'>
        <ScanTextIcon className='size-4' />
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
      >
        <Wand2Icon className='size-4' />
        {t('mask.inpaint')}
      </Button>

      <Separator orientation='vertical' className='mx-0.5 h-4' />

      <Button
        variant='ghost'
        size='xs'
        onClick={render}
        data-testid='toolbar-render'
      >
        <TypeIcon className='size-4' />
        {t('llm.render')}
      </Button>
    </div>
  )
}

export function RenderControls() {
  const renderEffect = useEditorUiStore((state) => state.renderEffect)
  const setRenderEffect = useEditorUiStore((state) => state.setRenderEffect)
  const { updateTextBlocks } = useTextBlockMutations()
  const { data: availableFonts = [] } = useFontsQuery()
  const fontFamily = usePreferencesStore((state) => state.fontFamily)
  const setFontFamily = usePreferencesStore((state) => state.setFontFamily)
  const { textBlocks, selectedBlockIndex, replaceBlock } = useTextBlocks()
  const { t } = useTranslation()
  const selectedBlock =
    selectedBlockIndex !== undefined
      ? textBlocks[selectedBlockIndex]
      : undefined
  const firstBlock = textBlocks[0]
  const hasBlocks = textBlocks.length > 0
  const fallbackFontFamilies =
    availableFonts.length > 0 ? [availableFonts[0]] : DEFAULT_FONT_FAMILIES
  const fallbackColor = firstBlock?.style?.color ?? DEFAULT_COLOR
  const fontCandidates =
    availableFonts.length > 0
      ? availableFonts
      : [
          ...(fontFamily ? [fontFamily] : []),
          ...(selectedBlock?.style?.fontFamilies?.slice(0, 1) ?? []),
          ...DEFAULT_FONT_FAMILIES,
        ]
  const fontOptions = uniqueStrings(fontCandidates)
  const currentFont =
    fontFamily ??
    selectedBlock?.style?.fontFamilies?.[0] ??
    firstBlock?.style?.fontFamilies?.[0] ??
    (hasBlocks ? fallbackFontFamilies[0] : '')
  const currentEffect = selectedBlock?.style?.effect ?? renderEffect
  const currentColor =
    selectedBlock?.style?.color ?? (hasBlocks ? fallbackColor : DEFAULT_COLOR)
  const currentColorHex = colorToHex(currentColor)

  const effects: { value: RenderEffect; label: string }[] = [
    { value: 'normal', label: t('render.effectNormal') },
    { value: 'antique', label: t('render.effectAntique') },
    { value: 'metal', label: t('render.effectMetal') },
    { value: 'manga', label: t('render.effectManga') },
    { value: 'motionBlur', label: t('render.effectMotionBlur') },
  ]

  const buildStyle = (
    style: TextStyle | undefined,
    updates: Partial<TextStyle>,
  ): TextStyle => ({
    fontFamilies:
      updates.fontFamilies ?? style?.fontFamilies ?? fallbackFontFamilies,
    fontSize: updates.fontSize ?? style?.fontSize,
    color: updates.color ?? style?.color ?? fallbackColor,
    effect: updates.effect ?? style?.effect,
  })

  const applyStyleToSelected = (updates: Partial<TextStyle>) => {
    if (selectedBlockIndex === undefined) return false
    const nextStyle = buildStyle(selectedBlock?.style, updates)
    void replaceBlock(selectedBlockIndex, { style: nextStyle })
    return true
  }

  const applyStyleToAll = (updates: Partial<TextStyle>) => {
    if (!hasBlocks) return
    const nextBlocks = textBlocks.map((block) => ({
      ...block,
      style: buildStyle(block.style, updates),
    }))
    void updateTextBlocks(nextBlocks)
  }

  const mergeFontFamilies = (
    nextFont: string,
    current: string[] | undefined,
  ) => {
    const base = current?.length ? current : fallbackFontFamilies
    return [nextFont, ...base.filter((family) => family !== nextFont)]
  }

  return (
    <div className='flex items-center gap-2'>
      <Select
        value={currentFont}
        onValueChange={(value) => {
          setFontFamily(value)
          const nextFamilies = mergeFontFamilies(
            value,
            selectedBlock?.style?.fontFamilies,
          )
          if (applyStyleToSelected({ fontFamilies: nextFamilies })) return
          if (!hasBlocks) return
          const nextBlocks = textBlocks.map((block) => ({
            ...block,
            style: buildStyle(block.style, {
              fontFamilies: mergeFontFamilies(value, block.style?.fontFamilies),
            }),
          }))
          void updateTextBlocks(nextBlocks)
        }}
        disabled={fontOptions.length === 0}
      >
        <SelectTrigger
          data-testid='render-font-select'
          size='sm'
          className='h-8 w-32 text-sm'
          style={currentFont ? { fontFamily: currentFont } : undefined}
        >
          <SelectValue placeholder={t('render.fontPlaceholder')} />
        </SelectTrigger>
        <SelectContent position='popper'>
          {fontOptions.map((font, index) => (
            <SelectItem
              key={font}
              value={font}
              style={{ fontFamily: font }}
              data-testid={`render-font-option-${index}`}
            >
              {font}
            </SelectItem>
          ))}
        </SelectContent>
      </Select>

      <Tooltip>
        <TooltipTrigger asChild>
          <div>
            <ColorPicker
              value={currentColorHex}
              disabled={!hasBlocks}
              triggerTestId='render-color-trigger'
              pickerTestId='render-color-picker'
              swatchTestId='render-color-swatch'
              onChange={(hex) => {
                const nextColor = hexToColor(hex, currentColor[3] ?? 255)
                if (applyStyleToSelected({ color: nextColor })) return
                applyStyleToAll({ color: nextColor })
              }}
              className='h-8 w-8'
            />
          </div>
        </TooltipTrigger>
        <TooltipContent side='bottom' sideOffset={4}>
          {t('render.fontColorLabel')}
        </TooltipContent>
      </Tooltip>

      <Select
        value={currentEffect}
        onValueChange={(value) => {
          const nextEffect = value as RenderEffect
          if (applyStyleToSelected({ effect: nextEffect })) return
          setRenderEffect(nextEffect)
        }}
      >
        <SelectTrigger
          data-testid='render-effect-select'
          size='sm'
          className='h-8 w-28 text-sm'
        >
          <SelectValue />
        </SelectTrigger>
        <SelectContent position='popper'>
          {effects.map((effect, index) => (
            <SelectItem
              key={effect.value}
              value={effect.value}
              data-testid={`render-effect-option-${index}`}
            >
              {effect.label}
            </SelectItem>
          ))}
        </SelectContent>
      </Select>
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

  const activeLanguages = useMemo(
    () =>
      llmModels.find((model) => model.id === llmSelectedModel)?.languages ?? [],
    [llmModels, llmSelectedModel],
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
        </button>
      </PopoverTrigger>
      <PopoverContent align='end' className='w-72' data-testid='llm-popover'>
        <div className='space-y-3 text-sm'>
          <p className='text-muted-foreground text-xs font-medium uppercase'>
            {t('panels.llm')}
          </p>

          {/* Model selector */}
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
                  {model.id}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>

          {/* Language selector */}
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
                    {language}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
          )}

          {/* Load/Unload button */}
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
