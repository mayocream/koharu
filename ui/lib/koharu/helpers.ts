import type { Element, ProjectId, TextBlock } from './protocol'

export function isTextElement(
  element: Element,
): element is Element & { kind: { Text: TextBlock } } {
  return 'Text' in element.kind
}

export function thumbnailUrl(project: ProjectId, blob: string, width = 160): string {
  const origin =
    typeof navigator !== 'undefined' && navigator.userAgent.includes('Windows')
      ? 'http://koharu-resource.project'
      : 'koharu-resource://project'
  return `${origin}/${project}/blob/${blob}?width=${width}`
}
