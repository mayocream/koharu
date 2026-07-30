import type { EntityView, ProjectId } from './protocol'

export function isTextElement(
  entity: EntityView,
): entity is EntityView & { source_text: NonNullable<EntityView['source_text']> } {
  return entity.source_text !== null
}

export function thumbnailUrl(project: ProjectId, blob: string, width = 160): string {
  const origin =
    typeof navigator !== 'undefined' && navigator.userAgent.includes('Windows')
      ? 'http://koharu-resource.project'
      : 'koharu-resource://project'
  return `${origin}/${project}/blob/${blob}?width=${width}`
}
