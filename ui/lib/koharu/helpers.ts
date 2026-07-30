import type { EntityView, ProjectId } from './protocol'

const TEXT_REGION_KIND = 'dev.koharu.region.text'

export function isTextElement(entity: EntityView): boolean {
  return (
    entity.source_text !== null ||
    entity.translation !== null ||
    entity.region?.kind === TEXT_REGION_KIND
  )
}

export function thumbnailUrl(project: ProjectId, blob: string, width = 160): string {
  const origin =
    typeof navigator !== 'undefined' && navigator.userAgent.includes('Windows')
      ? 'http://koharu-resource.project'
      : 'koharu-resource://project'
  return `${origin}/${project}/blob/${blob}?width=${width}`
}
