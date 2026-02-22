'use client'

import { useCallback, useEffect, useMemo } from 'react'
import { useAppStore, useConfigStore } from '@/lib/store'
import { useTextBlocks } from '@/hooks/useTextBlocks'
import type { RenderEffect, TextStyle } from '@/types'
import {
  DEFAULT_COLOR,
  DEFAULT_FONT_FAMILIES,
  RENDER_EFFECTS,
  buildTextStyle,
  colorToHex,
  hexToColor,
  mergeFontFamilies,
  uniqueStrings,
} from '@/features/editor-controls/render-style'

export function useRenderStyleControls() {
  const renderEffect = useAppStore((state) => state.renderEffect)
  const setRenderEffect = useAppStore((state) => state.setRenderEffect)
  const updateTextBlocks = useAppStore((state) => state.updateTextBlocks)
  const availableFonts = useAppStore((state) => state.availableFonts)
  const fetchAvailableFonts = useAppStore((state) => state.fetchAvailableFonts)
  const fontFamily = useConfigStore((state) => state.fontFamily)
  const setFontFamily = useConfigStore((state) => state.setFontFamily)

  const { textBlocks, selectedBlockIndex, replaceBlock } = useTextBlocks()
  const hasBlocks = textBlocks.length > 0
  const selectedBlock =
    selectedBlockIndex !== undefined
      ? textBlocks[selectedBlockIndex]
      : undefined
  const firstBlock = textBlocks[0]

  useEffect(() => {
    if (availableFonts.length === 0) {
      void fetchAvailableFonts()
    }
  }, [availableFonts.length, fetchAvailableFonts])

  const fallbackFontFamilies = useMemo(
    () =>
      availableFonts.length > 0 ? [availableFonts[0]] : DEFAULT_FONT_FAMILIES,
    [availableFonts],
  )
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

  const effects = RENDER_EFFECTS

  const toStyle = useCallback(
    (style: TextStyle | undefined, updates: Partial<TextStyle>): TextStyle =>
      buildTextStyle(style, updates, fallbackFontFamilies, fallbackColor),
    [fallbackColor, fallbackFontFamilies],
  )

  const applyStyleToSelected = useCallback(
    (updates: Partial<TextStyle>) => {
      if (selectedBlockIndex === undefined) return false
      const nextStyle = toStyle(selectedBlock?.style, updates)
      void replaceBlock(selectedBlockIndex, { style: nextStyle })
      return true
    },
    [replaceBlock, selectedBlock?.style, selectedBlockIndex, toStyle],
  )

  const applyStyleToAll = useCallback(
    (updates: Partial<TextStyle>) => {
      if (!hasBlocks) return
      const nextBlocks = textBlocks.map((block) => ({
        ...block,
        style: toStyle(block.style, updates),
      }))
      void updateTextBlocks(nextBlocks)
    },
    [hasBlocks, textBlocks, toStyle, updateTextBlocks],
  )

  const setFont = useCallback(
    (value: string) => {
      setFontFamily(value)
      const nextFamilies = mergeFontFamilies(
        value,
        selectedBlock?.style?.fontFamilies,
        fallbackFontFamilies,
      )
      if (applyStyleToSelected({ fontFamilies: nextFamilies })) return
      if (!hasBlocks) return
      const nextBlocks = textBlocks.map((block) => ({
        ...block,
        style: toStyle(block.style, {
          fontFamilies: mergeFontFamilies(
            value,
            block.style?.fontFamilies,
            fallbackFontFamilies,
          ),
        }),
      }))
      void updateTextBlocks(nextBlocks)
    },
    [
      applyStyleToSelected,
      fallbackFontFamilies,
      hasBlocks,
      selectedBlock?.style?.fontFamilies,
      setFontFamily,
      textBlocks,
      toStyle,
      updateTextBlocks,
    ],
  )

  const setColor = useCallback(
    (hex: string) => {
      const nextColor = hexToColor(hex, currentColor[3] ?? 255)
      if (applyStyleToSelected({ color: nextColor })) return
      applyStyleToAll({ color: nextColor })
    },
    [applyStyleToAll, applyStyleToSelected, currentColor],
  )

  const setEffect = useCallback(
    (value: string) => {
      const nextEffect = value as RenderEffect
      if (applyStyleToSelected({ effect: nextEffect })) return
      setRenderEffect(nextEffect)
    },
    [applyStyleToSelected, setRenderEffect],
  )

  return {
    hasBlocks,
    textBlocks,
    selectedBlockIndex,
    fontOptions,
    currentFont,
    currentEffect,
    currentColorHex,
    effects,
    setFont,
    setColor,
    setEffect,
  }
}
