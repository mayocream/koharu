import type { LlmTarget } from '@/lib/api/schemas'

export const llmTargetKey = (target: LlmTarget) =>
  target.kind === 'local'
    ? `local:${target.modelId}`
    : `provider:${target.providerId ?? ''}:${target.modelId}`

export const sameLlmTarget = (left?: LlmTarget | null, right?: LlmTarget | null) =>
  !!left &&
  !!right &&
  left.kind === right.kind &&
  left.modelId === right.modelId &&
  (left.providerId ?? null) === (right.providerId ?? null)
