import { z } from 'zod'

const jobStatusSchema = z.enum(['running', 'completed', 'cancelled', 'failed'])
const llmStatusSchema = z.enum(['empty', 'loading', 'ready', 'failed'])
const transferStatusSchema = z.enum([
  'started',
  'downloading',
  'completed',
  'failed',
])

export const documentSummarySchema = z
  .object({
    documentUrl: z.string(),
    hasBrushLayer: z.boolean(),
    hasInpainted: z.boolean(),
    hasRendered: z.boolean(),
    hasSegment: z.boolean(),
    height: z.number().int().nonnegative(),
    id: z.string(),
    name: z.string(),
    revision: z.number().int().nonnegative(),
    textBlockCount: z.number().int().nonnegative(),
    thumbnailUrl: z.string(),
    width: z.number().int().nonnegative(),
  })
  .passthrough()

export const documentsChangedEventSchema = z
  .object({
    documents: z.array(documentSummarySchema),
  })
  .passthrough()

export const documentChangedEventSchema = z
  .object({
    documentId: z.string(),
    revision: z.number().int().nonnegative(),
    changed: z.array(z.string()),
  })
  .passthrough()

export const downloadStateSchema = z
  .object({
    id: z.string(),
    filename: z.string(),
    downloaded: z.number().nonnegative(),
    total: z.number().nonnegative().nullable(),
    status: transferStatusSchema,
    error: z.string().nullable(),
  })
  .passthrough()

export const jobStateSchema = z
  .object({
    currentDocument: z.number().int().nonnegative(),
    currentStepIndex: z.number().int().nonnegative(),
    error: z.string().nullable().optional(),
    id: z.string(),
    kind: z.string(),
    overallPercent: z.number().nonnegative(),
    status: jobStatusSchema,
    step: z.string().nullable().optional(),
    totalDocuments: z.number().int().nonnegative(),
    totalSteps: z.number().int().nonnegative(),
  })
  .passthrough()

export const llmStateSchema = z
  .object({
    error: z.string().nullable().optional(),
    modelId: z.string().nullable().optional(),
    source: z.string().nullable().optional(),
    status: llmStatusSchema,
  })
  .passthrough()

export const snapshotEventSchema = z
  .object({
    documents: z.array(documentSummarySchema),
    llm: llmStateSchema,
    jobs: z.array(jobStateSchema),
    downloads: z.array(downloadStateSchema),
  })
  .passthrough()
