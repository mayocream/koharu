'use client'

import { PlusIcon, Trash2Icon } from 'lucide-react'
import { type FormEvent, type KeyboardEvent, useState } from 'react'

import { Button } from '@/components/ui/button'
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogTitle,
} from '@/components/ui/dialog'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { Separator } from '@/components/ui/separator'
import {
  WORKFLOW_STEP_KEYS,
  type PipelineStepKey,
  usePreferencesStore,
} from '@/lib/stores/preferencesStore'

type WorkflowPresetsDialogProps = {
  open: boolean
  onOpenChange: (open: boolean) => void
}

const STEP_LABELS: Record<PipelineStepKey, string> = {
  detect: 'Detect',
  ocr: 'OCR',
  translate: 'Translate',
  inpaint: 'Inpaint',
  render: 'Render',
}

const DEFAULT_SELECTED_STEPS: PipelineStepKey[] = ['detect', 'ocr']

function formatSteps(steps: PipelineStepKey[]): string {
  return steps.map((step) => STEP_LABELS[step]).join(' + ')
}

export function WorkflowPresetsDialog({ open, onOpenChange }: WorkflowPresetsDialogProps) {
  const workflowPresets = usePreferencesStore((s) => s.workflowPresets)
  const addWorkflowPreset = usePreferencesStore((s) => s.addWorkflowPreset)
  const removeWorkflowPreset = usePreferencesStore((s) => s.removeWorkflowPreset)
  const renameWorkflowPreset = usePreferencesStore((s) => s.renameWorkflowPreset)
  const [name, setName] = useState('')
  const [selectedSteps, setSelectedSteps] = useState<PipelineStepKey[]>(DEFAULT_SELECTED_STEPS)
  const [renameDrafts, setRenameDrafts] = useState<Record<string, string>>({})

  const canSave = name.trim().length > 0 && selectedSteps.length > 0

  const toggleStep = (step: PipelineStepKey) => {
    setSelectedSteps((steps) =>
      steps.includes(step) ? steps.filter((candidate) => candidate !== step) : [...steps, step],
    )
  }

  const clearRenameDraft = (id: string) => {
    setRenameDrafts((drafts) => {
      const next = { ...drafts }
      delete next[id]
      return next
    })
  }

  const commitRename = (id: string) => {
    const draft = renameDrafts[id]
    if (draft === undefined) return
    const nextName = draft.trim()
    if (nextName) renameWorkflowPreset(id, nextName)
    clearRenameDraft(id)
  }

  const handleRenameKeyDown = (event: KeyboardEvent<HTMLInputElement>, id: string) => {
    if (event.key === 'Enter') {
      event.preventDefault()
      commitRename(id)
      event.currentTarget.blur()
    }
    if (event.key === 'Escape') {
      clearRenameDraft(id)
      event.currentTarget.blur()
    }
  }

  const handleSubmit = (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault()
    if (!canSave) return
    addWorkflowPreset({ name, steps: selectedSteps })
    setName('')
    setSelectedSteps(DEFAULT_SELECTED_STEPS)
  }

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className='max-w-md gap-4'>
        <div className='space-y-1'>
          <DialogTitle>Workflow Presets</DialogTitle>
          <DialogDescription className='sr-only'>
            Create, rename, and delete workflow presets.
          </DialogDescription>
        </div>

        <form className='space-y-3' onSubmit={handleSubmit}>
          <div className='space-y-1.5'>
            <Label htmlFor='workflow-preset-name'>Preset name</Label>
            <Input
              id='workflow-preset-name'
              value={name}
              onChange={(event) => setName(event.target.value)}
              placeholder='Detect + OCR'
              data-testid='workflow-preset-name'
            />
          </div>

          <div className='grid grid-cols-2 gap-2'>
            {WORKFLOW_STEP_KEYS.map((step) => (
              <label
                key={step}
                className='flex h-9 items-center gap-2 rounded-md border border-input px-2 text-sm'
              >
                <input
                  type='checkbox'
                  className='size-4 accent-primary'
                  checked={selectedSteps.includes(step)}
                  onChange={() => toggleStep(step)}
                  data-testid={`workflow-preset-step-${step}`}
                />
                <span>{STEP_LABELS[step]}</span>
              </label>
            ))}
          </div>

          <div className='flex justify-end'>
            <Button
              type='submit'
              size='sm'
              disabled={!canSave}
              data-testid='workflow-preset-save'
            >
              <PlusIcon className='size-4' />
              Save preset
            </Button>
          </div>
        </form>

        <Separator />

        <div className='max-h-64 space-y-2 overflow-y-auto pr-1'>
          {workflowPresets.map((preset) => (
            <div
              key={preset.id}
              className='flex items-center gap-2 rounded-md border border-border px-2 py-2'
              data-testid={`workflow-preset-row-${preset.id}`}
            >
              <div className='min-w-0 flex-1 space-y-1'>
                <Input
                  value={renameDrafts[preset.id] ?? preset.name}
                  onChange={(event) =>
                    setRenameDrafts((drafts) => ({
                      ...drafts,
                      [preset.id]: event.target.value,
                    }))
                  }
                  onBlur={() => commitRename(preset.id)}
                  onKeyDown={(event) => handleRenameKeyDown(event, preset.id)}
                  aria-label={`Rename ${preset.name}`}
                  className='h-7 px-2 text-xs md:text-xs'
                  data-testid={`workflow-preset-name-${preset.id}`}
                />
                <div className='truncate px-1 text-[11px] text-muted-foreground'>
                  {formatSteps(preset.steps)}
                </div>
              </div>
              <Button
                variant='ghost'
                size='icon-xs'
                onClick={() => removeWorkflowPreset(preset.id)}
                aria-label={`Delete ${preset.name}`}
                data-testid={`workflow-preset-delete-${preset.id}`}
              >
                <Trash2Icon className='size-4' />
              </Button>
            </div>
          ))}
        </div>
      </DialogContent>
    </Dialog>
  )
}
