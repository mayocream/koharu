'use client'

import { Select } from 'radix-ui'
import { ToggleField, TooltipButton } from '@/components/ui/form-controls'
import { useTranslation } from 'react-i18next'
import { useAppStore } from '@/lib/store'
import { RenderEffect } from '@/types'

export function RenderControls() {
  const {
    showRenderedImage,
    setShowRenderedImage,
    render,
    renderEffect,
    setRenderEffect,
  } = useAppStore()
  const { t } = useTranslation()

  const effects: { value: RenderEffect; label: string }[] = [
    { value: 'normal', label: t('render.effectNormal') },
    { value: 'antique', label: t('render.effectAntique') },
    { value: 'metal', label: t('render.effectMetal') },
    { value: 'manga', label: t('render.effectManga') },
    { value: 'motionBlur', label: t('render.effectMotionBlur') },
  ]

  return (
    <div className='space-y-2 text-xs text-neutral-600'>
      <ToggleField
        label={t('mask.showRendered')}
        checked={showRenderedImage}
        onChange={setShowRenderedImage}
      />
      <div className='space-y-1'>
        <div className='text-[11px] font-semibold tracking-wide text-neutral-500 uppercase'>
          {t('render.effectLabel')}
        </div>
        <Select.Root
          value={renderEffect}
          onValueChange={(value) => setRenderEffect(value as RenderEffect)}
        >
          <Select.Trigger className='inline-flex w-full items-center justify-between gap-2 rounded border border-neutral-200 bg-white px-2 py-1 text-sm hover:bg-neutral-50'>
            <Select.Value />
          </Select.Trigger>
          <Select.Portal>
            <Select.Content className='min-w-56 rounded-md bg-white p-1 shadow-sm'>
              <Select.Viewport>
                {effects.map((effect) => (
                  <Select.Item
                    key={effect.value}
                    value={effect.value}
                    className='rounded px-3 py-1.5 text-sm outline-none select-none hover:bg-black/5 data-[state=checked]:bg-black/5'
                  >
                    <Select.ItemText>{effect.label}</Select.ItemText>
                  </Select.Item>
                ))}
              </Select.Viewport>
            </Select.Content>
          </Select.Portal>
        </Select.Root>
      </div>
      <div className='col flex'>
        <TooltipButton
          label={t('llm.render')}
          tooltip={t('llm.renderTooltip')}
          onClick={render}
          widthClass='w-full'
        />
      </div>
    </div>
  )
}
