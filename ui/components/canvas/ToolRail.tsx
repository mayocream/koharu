'use client'

import { Brush, Hand, MousePointer2, PanelLeft, ScanText, Type } from 'lucide-react'
import { useTranslation } from 'react-i18next'

import { Button } from '@/components/ui/button'
import { Tooltip, TooltipContent, TooltipTrigger } from '@/components/ui/tooltip'
import { useEditorStore, type EditorTool } from '@/lib/koharu'
import { cn } from '@/lib/utils'

const tools: { id: EditorTool; icon: typeof MousePointer2; label: string }[] = [
  { id: 'select', icon: MousePointer2, label: 'Select' },
  { id: 'text', icon: Type, label: 'Text' },
  { id: 'text_mask', icon: ScanText, label: 'Text mask' },
  { id: 'brush_mask', icon: Brush, label: 'Brush mask' },
  { id: 'pan', icon: Hand, label: 'Pan' },
]

export function ToolRail() {
  const { t } = useTranslation()
  const active = useEditorStore((state) => state.tool)
  const setTool = useEditorStore((state) => state.setTool)
  const shortcuts = useEditorStore((state) => state.shortcuts)
  const showNavigator = useEditorStore((state) => state.showNavigator)
  const setShowNavigator = useEditorStore((state) => state.setShowNavigator)
  return (
    <div className='pointer-events-auto z-20 flex w-11 shrink-0 flex-col border-r border-border bg-card'>
      <div className='flex h-[44px] shrink-0 items-center justify-center'>
        <Tooltip>
          <TooltipTrigger asChild>
            <Button
              size='icon-sm'
              variant='ghost'
              data-active={showNavigator}
              aria-label={showNavigator ? t('navigator.hide') : t('navigator.show')}
              aria-pressed={showNavigator}
              className='border border-transparent text-muted-foreground data-[active=true]:text-primary'
              onClick={() => setShowNavigator(!showNavigator)}
            >
              <PanelLeft className='size-4' />
            </Button>
          </TooltipTrigger>
          <TooltipContent side='right' sideOffset={8}>
            {showNavigator ? t('navigator.hide') : t('navigator.show')}
          </TooltipContent>
        </Tooltip>
      </div>
      <div className='h-px w-full bg-border' />
      <div className='flex flex-1 flex-col items-center gap-1 py-2'>
        {tools.map(({ id, icon: Icon, label }) => (
          <Tooltip key={id}>
            <TooltipTrigger asChild>
              <Button
                size='icon-sm'
                variant='ghost'
                data-active={active === id}
                aria-label={t(`native.tools.${id}`, { defaultValue: label })}
                className={cn(
                  'border border-transparent text-muted-foreground data-[active=true]:border-primary data-[active=true]:bg-accent data-[active=true]:text-primary',
                )}
                onClick={() => setTool(id)}
              >
                <Icon className='size-4' />
              </Button>
            </TooltipTrigger>
            <TooltipContent side='right' sideOffset={8}>
              {t(`native.tools.${id}`, { defaultValue: label })} ({shortcuts[id].toUpperCase()})
            </TooltipContent>
          </Tooltip>
        ))}
      </div>
    </div>
  )
}
