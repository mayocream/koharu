import { z } from 'zod'

const uint8ArrayInputSchema = z.custom<Uint8Array>(
  (value) => value instanceof Uint8Array,
  {
    message: 'Expected Uint8Array',
  },
)

const binaryArraySchema = z
  .array(z.number().int().min(0).max(255))
  .transform((value) => Uint8Array.from(value))

const decodeUtf8 = (value: Uint8Array): string => {
  try {
    return new TextDecoder().decode(value)
  } catch {
    return ''
  }
}

const decodePathUnits = (value: number[]): string => {
  if (value.length === 0) return ''
  const max = Math.max(...value)
  if (max <= 255) {
    return decodeUtf8(Uint8Array.from(value))
  }
  try {
    const units = Uint16Array.from(value)
    return new TextDecoder('utf-16le').decode(new Uint8Array(units.buffer))
  } catch {
    return value.map((unit) => String.fromCharCode(unit)).join('')
  }
}

const pathSchema = z
  .union([
    z.string(),
    uint8ArrayInputSchema,
    z.array(z.number().int().min(0).max(65535)),
  ])
  .transform((value) => {
    if (typeof value === 'string') return value
    if (value instanceof Uint8Array) return decodeUtf8(value)
    return decodePathUnits(value)
  })

const uint8ArraySchema = z.union([uint8ArrayInputSchema, binaryArraySchema])

const fromRustOption = <T extends z.ZodTypeAny>(schema: T) =>
  z.preprocess(
    (value) => (value === null ? undefined : value),
    schema.optional(),
  )

export const rgbaColorSchema = z.tuple([
  z.number().int().min(0).max(255),
  z.number().int().min(0).max(255),
  z.number().int().min(0).max(255),
  z.number().int().min(0).max(255),
])

export const renderEffectSchema = z.enum([
  'normal',
  'antique',
  'metal',
  'manga',
  'motionBlur',
])

export const textStyleSchema = z.object({
  fontFamilies: z.array(z.string()),
  fontSize: fromRustOption(z.number()),
  color: rgbaColorSchema,
  effect: fromRustOption(renderEffectSchema),
})

const namedFontPredictionSchema = z.object({
  index: z.number(),
  name: z.string(),
  language: fromRustOption(z.string()),
  probability: z.number(),
  serif: z.boolean(),
})

const fontPredictionSchema = z
  .object({
    top_fonts: z.array(z.tuple([z.number(), z.number()])),
    named_fonts: z.array(namedFontPredictionSchema),
    direction: z.enum(['Horizontal', 'Vertical']),
    text_color: z.tuple([
      z.number().int().min(0).max(255),
      z.number().int().min(0).max(255),
      z.number().int().min(0).max(255),
    ]),
    stroke_color: z.tuple([
      z.number().int().min(0).max(255),
      z.number().int().min(0).max(255),
      z.number().int().min(0).max(255),
    ]),
    font_size_px: z.number(),
    stroke_width_px: z.number(),
    line_height: z.number(),
    angle_deg: z.number(),
  })
  .passthrough()

export const textBlockSchema = z.object({
  x: z.number(),
  y: z.number(),
  width: z.number(),
  height: z.number(),
  confidence: z.number(),
  text: fromRustOption(z.string()),
  translation: fromRustOption(z.string()),
  style: fromRustOption(textStyleSchema),
  fontPrediction: fromRustOption(fontPredictionSchema),
  rendered: fromRustOption(uint8ArraySchema),
})

export const documentSchema = z.object({
  id: z.string(),
  path: pathSchema,
  name: z.string(),
  image: uint8ArraySchema,
  width: z.number(),
  height: z.number(),
  textBlocks: z.array(textBlockSchema),
  segment: fromRustOption(uint8ArraySchema),
  inpainted: fromRustOption(uint8ArraySchema),
  brushLayer: fromRustOption(uint8ArraySchema),
  rendered: fromRustOption(uint8ArraySchema),
})

export const llmModelInfoSchema = z.object({
  id: z.string(),
  languages: z.array(z.string()),
})

export const llmModelInfoListSchema = z.array(llmModelInfoSchema)

export const inpaintRegionSchema = z.object({
  x: z.number(),
  y: z.number(),
  width: z.number(),
  height: z.number(),
})

export const fileResultSchema = z.object({
  filename: z.string(),
  data: uint8ArraySchema,
  contentType: z.string(),
})

export const thumbnailResultSchema = z.object({
  data: uint8ArraySchema,
  contentType: z.string(),
})

export const downloadProgressSchema = z.object({
  filename: z.string(),
  downloaded: z.number(),
  total: fromRustOption(z.number()),
  status: z.union([
    z.literal('started'),
    z.literal('downloading'),
    z.literal('completed'),
    z.object({ failed: z.string() }),
  ]),
})

export const processProgressSchema = z.object({
  status: z.union([
    z.literal('running'),
    z.literal('completed'),
    z.literal('cancelled'),
    z.object({ failed: z.string() }),
  ]),
  step: z.string().nullable(),
  currentDocument: z.number(),
  totalDocuments: z.number(),
  currentStepIndex: z.number(),
  totalSteps: z.number(),
  overallPercent: z.number(),
})

export const deviceInfoSchema = z.object({
  mlDevice: z.string(),
  wgpu: z.object({
    name: z.string(),
    backend: z.string(),
    deviceType: z.string(),
    driver: z.string(),
    driverInfo: z.string(),
  }),
})
