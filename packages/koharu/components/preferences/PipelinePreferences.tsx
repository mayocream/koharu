'use client'

import { Eraser, FileText, Search } from 'lucide-react'

import {
  defaultModel,
  modelNames,
  modelOptions,
  replaceStage,
  stageModel,
  type ModelName,
  type ModelStage,
  type PipelineModel,
} from '@/components/preferences/models'
import {
  NumberField,
  PreferencePage,
  PreferenceRow,
  PreferenceSection,
  TextField,
} from '@/components/preferences/PreferenceFields'
import type { PipelineConfig } from '@/lib/protocol'
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@koharu/ui/components/select'

const stages = [
  ['detection', Search, 'Detection', 'Find panels, bubbles, and text regions.'],
  ['ocr', FileText, 'Text recognition', 'Read source text from detected regions.'],
  ['inpainting', Eraser, 'Artwork cleanup', 'Reconstruct artwork beneath the original text.'],
] as const satisfies ReadonlyArray<readonly [ModelStage, typeof Search, string, string]>

export function PipelinePreferences({
  value,
  onChange,
}: {
  value: PipelineConfig
  onChange: (value: PipelineConfig) => void
}) {
  return (
    <PreferencePage
      title='Pipeline'
      description='Choose the models used for page analysis and cleanup. Koharu downloads them when they are first needed.'
    >
      <PreferenceSection title='Page processing'>
        {stages.map(([stage, Icon, title, description]) => {
          const model = stageModel(value, stage)
          return (
            <PreferenceRow key={stage} title={title} description={description} align='start'>
              <div className='grid gap-3'>
                <div className='flex items-center gap-2'>
                  <Icon className='size-3.5 shrink-0 text-muted-foreground' />
                  <Select
                    value={model.model}
                    items={Object.fromEntries(
                      modelOptions[stage].map((name) => [name, modelNames[name]]),
                    )}
                    onValueChange={(name) => {
                      if (name) {
                        onChange(replaceStage(value, stage, defaultModel(name as ModelName)))
                      }
                    }}
                  >
                    <SelectTrigger
                      aria-label={`${title} model`}
                      className='h-8 min-w-0 flex-1 text-[11px]'
                    >
                      <SelectValue />
                    </SelectTrigger>
                    <SelectContent>
                      {modelOptions[stage].map((name) => (
                        <SelectItem key={name} value={name}>
                          {modelNames[name]}
                        </SelectItem>
                      ))}
                    </SelectContent>
                  </Select>
                </div>
                <ModelOptions
                  model={model}
                  onChange={(next) => onChange(replaceStage(value, stage, next))}
                />
              </div>
            </PreferenceRow>
          )
        })}
      </PreferenceSection>
    </PreferencePage>
  )
}

function ModelOptions({
  model,
  onChange,
}: {
  model: PipelineModel
  onChange: (model: PipelineModel) => void
}) {
  switch (model.model) {
    case 'koharu-layout-rfdetr-seg-2xl':
      return (
        <div className='grid grid-cols-3 gap-2'>
          <NumberField
            label='Text threshold'
            value={model.text_threshold ?? null}
            min={0}
            max={1}
            step={0.05}
            onChange={(text_threshold) => onChange({ ...model, text_threshold })}
          />
          <NumberField
            label='Bubble threshold'
            value={model.bubble_threshold ?? null}
            min={0}
            max={1}
            step={0.05}
            onChange={(bubble_threshold) => onChange({ ...model, bubble_threshold })}
          />
          <NumberField
            label='Panel threshold'
            value={model.panel_threshold ?? null}
            min={0}
            max={1}
            step={0.05}
            onChange={(panel_threshold) => onChange({ ...model, panel_threshold })}
          />
        </div>
      )
    case 'flux2-klein':
      return (
        <TextField
          label='Prompt'
          value={model.prompt ?? 'Remove the text and reconstruct the background.'}
          onChange={(prompt) => onChange({ ...model, prompt })}
        />
      )
    case 'rorem-mixed':
      return (
        <div className='grid grid-cols-2 gap-2'>
          <TextField
            label='Prompt'
            value={model.prompt ?? ''}
            onChange={(prompt) => onChange({ ...model, prompt })}
          />
          <TextField
            label='Negative prompt'
            value={model.negative_prompt ?? ''}
            onChange={(negative_prompt) => onChange({ ...model, negative_prompt })}
          />
        </div>
      )
    case 'paddleocr-vl-1.6':
    case 'manga-ocr':
    case 'baberu-ocr':
    case 'lama':
    case 'aot-inpainting':
      return null
  }
}
