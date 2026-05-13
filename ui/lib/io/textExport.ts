import type { Scene } from '@/lib/api/schemas'

// ── Types ───

type TextKindData = {
  text?: string | null
  translation?: string | null
  speaker?: string | null
}

export type ExportBlock = {
  index: number
  speaker: string | null
  ocr: string | null
  translation: string | null
}

export type ExportPage = {
  pageId: string
  pageName: string
  pageIndex: number
  blocks: ExportBlock[]
}

export type ExportData = {
  project: string
  exportedAt: string
  scope: 'current' | 'selected' | 'all'
  pages: ExportPage[]
}

// ── Data Collection ───

function extractTextBlocks(page: Scene['pages'][string]): ExportBlock[] {
  return Object.values(page.nodes)
    .filter((node) => 'text' in node.kind)
    .map((node, i) => {
      const td = (node.kind as { text: TextKindData }).text
      return {
        index: i + 1,
        speaker: td.speaker ?? null,
        ocr: td.text ?? null,
        translation: td.translation ?? null,
      }
    })
    .filter((b) => b.ocr || b.translation)
}

export function buildExportData(
  scene: Scene,
  pageIds: string[],
  scope: ExportData['scope'],
): ExportData {
  const pages: ExportPage[] = pageIds
    .map((pid, i) => {
      const page = scene.pages[pid]
      if (!page) return null
      return {
        pageId: pid,
        pageName: page.name,
        pageIndex: i + 1,
        blocks: extractTextBlocks(page),
      }
    })
    .filter((p): p is ExportPage => p !== null)

  return {
    project: scene.project?.name ?? '',
    exportedAt: new Date().toISOString(),
    scope,
    pages,
  }
}

// ── Serialization ───

export function toJson(data: ExportData): string {
  return JSON.stringify(data, null, 2)
}

export type ExportTKeys =
  | 'export.noText'
  | 'export.speaker'
  | 'export.original'
  | 'export.translation'

export type ExportTranslate = (key: ExportTKeys) => string

export function toTxt(data: ExportData, t: ExportTranslate): string {
  return data.pages
    .map((page) => {
      const header = page.pageName
      const body =
        page.blocks.length === 0
          ? t('export.noText')
          : page.blocks
              .map((b) => {
                const lines: string[] = [`[${b.index}]`]
                if (b.speaker)     lines.push(`{${t('export.speaker')}: ${b.speaker}}`)
                if (b.ocr)         lines.push(`{${t('export.original')}}\n${b.ocr}`)
                if (b.translation) lines.push(`{${t('export.translation')}}\n${b.translation}`)
                return lines.join('\n')
              })
              .join('\n\n')
      return `${header}\n${'─'.repeat(32)}\n${body}`
    })
    .join('\n\n' + '═'.repeat(40) + '\n\n')
}