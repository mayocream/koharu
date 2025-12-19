'use client'

import { useState } from 'react'
import { ToggleField, TooltipButton } from '@/components/ui/form-controls'
import { useTranslation } from 'react-i18next'
import { useAppStore } from '@/lib/store'

export function RenderControls() {
  const { showRenderedImage, setShowRenderedImage, render } = useAppStore()
  const { t } = useTranslation()

  return (
    <div className='space-y-2 text-xs text-neutral-600'>
      <ToggleField
        label={t('mask.showRendered')}
        checked={showRenderedImage}
        onChange={setShowRenderedImage}
      />
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
