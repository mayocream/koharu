import { describe, expect, it } from 'vitest'
import type { DocumentResource } from '@/lib/contracts/protocol'
import type { TextBlock, TextStyle } from '@/types'
import {
  buildTextBlockPatch,
  isTempTextBlockId,
  mapTextStyleForPatch,
  textBlockAliasKey,
} from './text-block-sync'

const baseStyle: TextStyle = {
  fontFamilies: ['Noto Sans'],
  fontSize: 24,
  color: [12, 34, 56, 255],
  effect: {
    italic: false,
    bold: true,
  },
  stroke: {
    enabled: true,
    color: [255, 255, 255, 255],
    widthPx: 2,
  },
  textAlign: 'center',
}

const previousBlock: DocumentResource['textBlocks'][number] = {
  id: 'block-1',
  x: 10,
  y: 20,
  width: 120,
  height: 42,
  confidence: 0.9,
  linePolygons: null,
  sourceDirection: null,
  renderedDirection: null,
  sourceLanguage: null,
  rotationDeg: null,
  detectedFontSizePx: null,
  detector: null,
  text: 'source text',
  translation: 'translated text',
  style: baseStyle,
  fontPrediction: null,
}

const nextBlock: TextBlock = {
  id: 'block-1',
  x: 10,
  y: 20,
  width: 120,
  height: 42,
  confidence: 0.9,
  text: 'source text',
  translation: 'translated text',
  style: baseStyle,
}

describe('text block sync', () => {
  it('returns null when a block is unchanged', () => {
    expect(buildTextBlockPatch(nextBlock, previousBlock)).toBeNull()
  })

  it('builds a minimal patch for changed fields', () => {
    const changed = {
      ...nextBlock,
      x: 14,
      translation: 'updated translation',
      style: {
        ...baseStyle,
        fontSize: 28,
      },
    } satisfies TextBlock

    expect(buildTextBlockPatch(changed, previousBlock)).toEqual({
      translation: 'updated translation',
      x: 14,
      text: undefined,
      y: undefined,
      width: undefined,
      height: undefined,
      style: mapTextStyleForPatch(changed.style),
    })
  })

  it('recognizes temporary ids and generates alias keys', () => {
    expect(isTempTextBlockId('temp:block-1')).toBe(true)
    expect(isTempTextBlockId('block-1')).toBe(false)
    expect(textBlockAliasKey('doc-1', 'temp:block-1')).toBe(
      'doc-1:temp:block-1',
    )
  })
})
