'use client'

import { useEffect, useState } from 'react'
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
import { useAppStore } from '@/lib/store'
import { useTextBlocks } from '@/hooks/useTextBlocks'
import { OPENAI_COMPATIBLE_MODEL_ID } from '@/lib/openai'
import { RenderEffect, RgbaColor, TextStyle } from '@/types'
import { Button } from '@/components/ui/button'
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
  const { inpaint, detect, ocr, render, llmReady, llmGenerate } = useAppStore()
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
      <Button variant='ghost' size='xs' onClick={detect}>
        <ScanIcon className='size-4' />
        {t('processing.detect')}
      </Button>

      <Separator orientation='vertical' className='mx-0.5 h-4' />

      <Button variant='ghost' size='xs' onClick={ocr}>
        <ScanTextIcon className='size-4' />
        {t('processing.ocr')}
      </Button>

      <Separator orientation='vertical' className='mx-0.5 h-4' />

      <Button
        variant='ghost'
        size='xs'
        onClick={handleTranslate}
        disabled={!llmReady || generating}
      >
        {generating ? (
          <LoaderCircleIcon className='size-4 animate-spin' />
        ) : (
          <LanguagesIcon className='size-4' />
        )}
        {t('llm.generate')}
      </Button>

      <Separator orientation='vertical' className='mx-0.5 h-4' />

      <Button variant='ghost' size='xs' onClick={inpaint}>
        <Wand2Icon className='size-4' />
        {t('mask.inpaint')}
      </Button>

      <Separator orientation='vertical' className='mx-0.5 h-4' />

      <Button variant='ghost' size='xs' onClick={render}>
        <TypeIcon className='size-4' />
        {t('llm.render')}
      </Button>
    </div>
  )
}

export function RenderControls() {
  const {
    renderEffect,
    setRenderEffect,
    updateTextBlocks,
    availableFonts,
    fetchAvailableFonts,
  } = useAppStore()
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
          ...(selectedBlock?.style?.fontFamilies?.slice(0, 1) ?? []),
          ...DEFAULT_FONT_FAMILIES,
        ]
  const fontOptions = uniqueStrings(fontCandidates)
  const currentFont =
    selectedBlock?.style?.fontFamilies?.[0] ??
    firstBlock?.style?.fontFamilies?.[0] ??
    (hasBlocks ? fallbackFontFamilies[0] : '')
  const currentEffect = selectedBlock?.style?.effect ?? renderEffect
  const currentColor =
    selectedBlock?.style?.color ?? (hasBlocks ? fallbackColor : DEFAULT_COLOR)
  const currentColorHex = colorToHex(currentColor)

  useEffect(() => {
    if (availableFonts.length === 0) {
      fetchAvailableFonts()
    }
  }, [availableFonts.length, fetchAvailableFonts])

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
        disabled={!hasBlocks || fontOptions.length === 0}
      >
        <SelectTrigger
          size='sm'
          className='h-8 w-32 text-sm'
          style={currentFont ? { fontFamily: currentFont } : undefined}
        >
          <SelectValue placeholder={t('render.fontPlaceholder')} />
        </SelectTrigger>
        <SelectContent>
          {fontOptions.map((font) => (
            <SelectItem key={font} value={font} style={{ fontFamily: font }}>
              {font}
            </SelectItem>
          ))}
        </SelectContent>
      </Select>

      <Tooltip>
        <TooltipTrigger asChild>
          <label className='border-input hover:border-border flex h-8 w-8 cursor-pointer items-center justify-center rounded border transition'>
            <input
              type='color'
              value={currentColorHex}
              disabled={!hasBlocks}
              onChange={(event) => {
                const nextColor = hexToColor(
                  event.target.value,
                  currentColor[3] ?? 255,
                )
                if (applyStyleToSelected({ color: nextColor })) return
                applyStyleToAll({ color: nextColor })
              }}
              className='size-5 cursor-pointer appearance-none border-none p-0 disabled:cursor-not-allowed disabled:opacity-60'
            />
          </label>
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
        <SelectTrigger size='sm' className='h-8 w-28 text-sm'>
          <SelectValue />
        </SelectTrigger>
        <SelectContent>
          {effects.map((effect) => (
            <SelectItem key={effect.value} value={effect.value}>
              {effect.label}
            </SelectItem>
          ))}
        </SelectContent>
      </Select>
    </div>
  )
}

function LlmStatusPopover() {
  const {
    llmModels,
    llmSelectedModel,
    llmSelectedLanguage,
    llmReady,
    llmLoading,
    llmList,
    llmSetSelectedModel,
    llmSetSelectedLanguage,
    llmToggleLoadUnload,
    llmCheckReady,
    llmOpenAIEndpoint,
    llmOpenAIApiKey,
    llmOpenAIPrompt,
    llmOpenAIModel,
    setLlmOpenAIEndpoint,
    setLlmOpenAIApiKey,
    setLlmOpenAIPrompt,
    setLlmOpenAIModel,
  } = useAppStore()
  const { t } = useTranslation()

  const activeLanguages =
    llmModels.find((model) => model.id === llmSelectedModel)?.languages ?? []
  const isOpenAICompatible = llmSelectedModel === OPENAI_COMPATIBLE_MODEL_ID

  useEffect(() => {
    llmList()
    llmCheckReady()
    const interval = setInterval(llmCheckReady, 1500)
    return () => clearInterval(interval)
  }, [llmList, llmCheckReady])

  return (
    <Popover>
      <PopoverTrigger asChild>
        <button
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
      <PopoverContent align='end' className='w-72'>
        <div className='space-y-3 text-sm'>
          <p className='text-muted-foreground text-xs font-medium uppercase'>
            {t('panels.llm')}
          </p>

          {/* Model selector */}
          <Select value={llmSelectedModel} onValueChange={llmSetSelectedModel}>
            <SelectTrigger className='w-full'>
              <SelectValue placeholder={t('llm.selectPlaceholder')} />
            </SelectTrigger>
            <SelectContent>
              {llmModels.map((model) => (
                <SelectItem key={model.id} value={model.id}>
                  {model.id === OPENAI_COMPATIBLE_MODEL_ID
                    ? t('llm.openaiCompatible')
                    : model.id}
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
              <SelectTrigger className='w-full'>
                <SelectValue placeholder={t('llm.languagePlaceholder')} />
              </SelectTrigger>
              <SelectContent>
                {activeLanguages.map((language) => (
                  <SelectItem key={language} value={language}>
                    {language}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
          )}

          {/* OpenAI compatible settings */}
          {isOpenAICompatible && (
            <div className='space-y-1.5 rounded border p-1.5'>
              <input
                type='text'
                value={llmOpenAIEndpoint}
                placeholder={t('llm.openaiEndpointPlaceholder')}
                onChange={(event) => setLlmOpenAIEndpoint(event.target.value)}
                className='border-input bg-background text-foreground focus:border-primary h-6 w-full rounded border px-2 text-xs outline-none'
              />
              <input
                type='password'
                value={llmOpenAIApiKey}
                placeholder={t('llm.openaiApiKeyPlaceholder')}
                autoComplete='off'
                onChange={(event) => setLlmOpenAIApiKey(event.target.value)}
                className='border-input bg-background text-foreground focus:border-primary h-6 w-full rounded border px-2 text-xs outline-none'
              />
              <input
                value={llmOpenAIModel}
                placeholder={t('llm.openaiModelPlaceholder')}
                onChange={(event) => setLlmOpenAIModel(event.target.value)}
                className='border-input bg-background text-foreground focus:border-primary h-6 w-full rounded border px-2 text-xs outline-none'
              />
              <textarea
                value={llmOpenAIPrompt}
                placeholder={t('llm.openaiPromptLabel')}
                rows={2}
                onChange={(event) => setLlmOpenAIPrompt(event.target.value)}
                className='border-input bg-background text-foreground focus:border-primary w-full rounded border px-2 py-1 text-xs outline-none'
              />
            </div>
          )}

          {/* Load/Unload button */}
          {!isOpenAICompatible && (
            <Button
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
          )}
        </div>
      </PopoverContent>
    </Popover>
  )
}
