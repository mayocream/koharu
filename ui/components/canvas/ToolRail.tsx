'use client'

import { MousePointer, VectorSquare, Brush, Bandage, Eraser, PanelLeft } from 'lucide-react'
import type { ComponentType } from 'react'
import { useTranslation } from 'react-i18next'

import { Button } from '@/components/ui/button'
import { Tooltip, TooltipContent, TooltipTrigger } from '@/components/ui/tooltip'
import { useEditorUiStore } from '@/lib/stores/editorUiStore'
import { usePreferencesStore } from '@/lib/stores/preferencesStore'
import type { ToolMode } from '@/lib/types'

type ModeDefinition = {
  value: ToolMode
  icon: ComponentType<{ className?: string }>
  labelKey: string
  testId: string
}

const MODES: ModeDefinition[] = [
  {
    labelKey: 'toolRail.select',
    value: 'select',
    icon: MousePointer,
    testId: 'tool-select',
  },
  {
    labelKey: 'toolRail.block',
    value: 'block',
    icon: VectorSquare,
    testId: 'tool-block',
  },
  {
    labelKey: 'toolRail.brush',
    value: 'brush',
    icon: Brush,
    testId: 'tool-brush',
  },
  {
    labelKey: 'toolRail.eraser',
    value: 'eraser',
    icon: Eraser,
    testId: 'tool-eraser',
  },
  {
    labelKey: 'toolRail.repairBrush',
    value: 'repairBrush',
    icon: Bandage,
    testId: 'tool-repairBrush',
  },
]

export function ToolRail() {
  const mode = useEditorUiStore((state) => state.mode)
  const setMode = useEditorUiStore((state) => state.setMode)
  const showNavigator = useEditorUiStore((state) => state.showNavigator)
  const setShowNavigator = useEditorUiStore((state) => state.setShowNavigator)
  const shortcuts = usePreferencesStore((state) => state.shortcuts)
  const { t } = useTranslation()

  return (
    <div className='flex w-11 flex-col border-r border-border bg-card'>
      {/* Navigator Toggle Section - Height matches Navigator Header (py-1.5 + 2 lines of text-xs = 44px) */}
      <div className='flex h-[44px] shrink-0 items-center justify-center'>
        <Tooltip>
          <TooltipTrigger asChild>
            <Button
              variant='ghost'
              size='icon-sm'
              data-testid='tool-navigator-toggle'
              data-active={showNavigator}
              onClick={() => setShowNavigator(!showNavigator)}
              className='border border-transparent text-muted-foreground data-[active=true]:text-primary'
              aria-label={showNavigator ? t('navigator.hide') : t('navigator.show')}
              aria-pressed={showNavigator}
            >
              <PanelLeft className='h-4 w-4' />
            </Button>
          </TooltipTrigger>
          <TooltipContent side='right' sideOffset={8}>
            {showNavigator ? t('navigator.hide') : t('navigator.show')}
          </TooltipContent>
        </Tooltip>
      </div>

      <div className='h-px w-full bg-border' />

      <div className='flex flex-1 flex-col items-center gap-1 py-2'>
        {MODES.map((item) => {
          const label = t(item.labelKey)

          return (
            <Tooltip key={item.value}>
              <TooltipTrigger asChild>
                <Button
                  variant='ghost'
                  size='icon-sm'
                  data-testid={item.testId}
                  data-active={item.value === mode}
                  onClick={() => setMode(item.value)}
                  className='border border-transparent text-muted-foreground data-[active=true]:border-primary data-[active=true]:bg-accent data-[active=true]:text-primary'
                  aria-label={label}
                >
                  <item.icon className='h-4 w-4' />
                </Button>
              </TooltipTrigger>
              <TooltipContent side='right' sideOffset={8}>
                {shortcuts[item.value as keyof typeof shortcuts]
                  ? `${label} (${shortcuts[item.value as keyof typeof shortcuts].toUpperCase()})`
                  : label}
              </TooltipContent>
            </Tooltip>
          )
        })}
      </div>
    </div>
  )
}
