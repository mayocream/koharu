'use client'

import {
  Bandage,
  Brush,
  Eraser,
  MousePointer,
  PanelLeft,
  SlidersHorizontal,
  VectorSquare,
} from 'lucide-react'
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

const isBrushLikeTool = (value: ToolMode) =>
  value === 'brush' || value === 'eraser' || value === 'repairBrush'

export function ToolRail() {
  const mode = useEditorUiStore((state) => state.mode)
  const setMode = useEditorUiStore((state) => state.setMode)
  const toolOptionsOpen = useEditorUiStore((state) => state.toolOptionsOpen)
  const setToolOptionsOpen = useEditorUiStore((state) => state.setToolOptionsOpen)
  const toggleToolOptionsOpen = useEditorUiStore((state) => state.toggleToolOptionsOpen)
  const showNavigator = useEditorUiStore((state) => state.showNavigator)
  const setShowNavigator = useEditorUiStore((state) => state.setShowNavigator)
  const shortcuts = usePreferencesStore((state) => state.shortcuts)

  const { t } = useTranslation()

  const handleToolClick = (value: ToolMode) => {
    if (isBrushLikeTool(value)) {
      if (value === mode) {
        toggleToolOptionsOpen()
        return
      }

      setMode(value)
      setToolOptionsOpen(true)
      return
    }

    setMode(value)
    setToolOptionsOpen(false)
  }

  return (
    <div className='flex w-12 flex-col border-r border-border bg-card/95 shadow-sm'>
      <div className='flex h-[44px] shrink-0 items-center justify-center'>
        <Tooltip>
          <TooltipTrigger asChild>
            <Button
              variant='ghost'
              size='icon-sm'
              data-testid='tool-navigator-toggle'
              data-active={showNavigator}
              onClick={() => setShowNavigator(!showNavigator)}
              className='h-8 w-8 rounded-lg border border-transparent text-muted-foreground transition hover:border-border/70 hover:bg-muted/60 hover:text-foreground data-[active=true]:bg-muted/50 data-[active=true]:text-primary'
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

      <div className='mx-2 h-px bg-border/80' />

      <div className='flex flex-1 flex-col items-center gap-1 py-2'>
        {MODES.map((item) => {
          const label = t(item.labelKey)
          const active = item.value === mode
          const optionsActive = active && isBrushLikeTool(item.value) && toolOptionsOpen

          return (
            <Tooltip key={item.value}>
              <TooltipTrigger asChild>
                <Button
                  variant='ghost'
                  size='icon-sm'
                  data-testid={item.testId}
                  data-active={active}
                  data-options-open={optionsActive}
                  onClick={() => handleToolClick(item.value)}
                  className='group relative h-8 w-8 rounded-lg border border-transparent text-muted-foreground transition hover:border-border/70 hover:bg-muted/60 hover:text-foreground data-[active=true]:border-primary/35 data-[active=true]:bg-primary/10 data-[active=true]:text-primary'
                  aria-label={label}
                  aria-pressed={active}
                >
                  <item.icon className='h-4 w-4' />

                  {optionsActive && (
                    <span
                      className='absolute -bottom-0.5 -right-0.5 flex h-3.5 w-3.5 items-center justify-center rounded-full border border-border bg-card text-primary shadow-sm'
                      aria-hidden='true'
                    >
                      <SlidersHorizontal className='h-2.5 w-2.5' />
                    </span>
                  )}
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
