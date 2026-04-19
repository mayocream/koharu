'use client'

import {
  EyeIcon,
  EyeOffIcon,
  SparklesIcon,
  ALargeSmallIcon,
  ContrastIcon,
  BandageIcon,
  PaintbrushIcon,
} from 'lucide-react'
import { motion } from 'motion/react'
import { useTranslation } from 'react-i18next'

import { Button } from '@/components/ui/button'
import { findImageBlob, findMaskBlob, useCurrentPage, useTextNodes } from '@/hooks/useCurrentPage'
import { useScene } from '@/hooks/useScene'
import { useEditorUiStore } from '@/lib/stores/editorUiStore'
import { cn } from '@/lib/utils'

type Layer = {
  id: string
  labelKey: string
  icon: React.ComponentType<{ className?: string }> | 'RAW'
  visible: boolean
  setVisible: (visible: boolean) => void
  hasContent: boolean
  alwaysEnabled?: boolean
}

export function LayersPanel() {
  const page = useCurrentPage()
  const { epoch: sceneEpoch } = useScene()
  const textNodes = useTextNodes()
  const showInpaintedImage = useEditorUiStore((s) => s.showInpaintedImage)
  const setShowInpaintedImage = useEditorUiStore((s) => s.setShowInpaintedImage)
  const showSegmentationMask = useEditorUiStore((s) => s.showSegmentationMask)
  const setShowSegmentationMask = useEditorUiStore((s) => s.setShowSegmentationMask)
  const showBrushLayer = useEditorUiStore((s) => s.showBrushLayer)
  const setShowBrushLayer = useEditorUiStore((s) => s.setShowBrushLayer)
  const showTextBlocksOverlay = useEditorUiStore((s) => s.showTextBlocksOverlay)
  const setShowTextBlocksOverlay = useEditorUiStore((s) => s.setShowTextBlocksOverlay)
  const showRenderedImage = useEditorUiStore((s) => s.showRenderedImage)
  const setShowRenderedImage = useEditorUiStore((s) => s.setShowRenderedImage)

  const hasRendered = !!(page && findImageBlob(page, 'rendered'))
  const hasInpainted = !!(page && findImageBlob(page, 'inpainted'))
  const hasSource = !!(page && findImageBlob(page, 'source'))
  const hasSegment = !!(page && findMaskBlob(page, 'segment'))
  const hasBrush = !!(page && findMaskBlob(page, 'brushInpaint'))
  // Silence warning about unused epoch dep — it's the invalidation trigger.
  void sceneEpoch

  const layers: Layer[] = [
    {
      id: 'rendered',
      labelKey: 'layers.rendered',
      icon: SparklesIcon,
      visible: showRenderedImage,
      setVisible: setShowRenderedImage,
      hasContent: hasRendered,
    },
    {
      id: 'textBlocks',
      labelKey: 'layers.textBlocks',
      icon: ALargeSmallIcon,
      visible: showTextBlocksOverlay,
      setVisible: setShowTextBlocksOverlay,
      hasContent: textNodes.length > 0,
    },
    {
      id: 'brush',
      labelKey: 'layers.brush',
      icon: PaintbrushIcon,
      visible: showBrushLayer,
      setVisible: setShowBrushLayer,
      hasContent: hasBrush,
    },
    {
      id: 'inpainted',
      labelKey: 'layers.inpainted',
      icon: BandageIcon,
      visible: showInpaintedImage,
      setVisible: setShowInpaintedImage,
      hasContent: hasInpainted,
    },
    {
      id: 'mask',
      labelKey: 'layers.mask',
      icon: ContrastIcon,
      visible: showSegmentationMask,
      setVisible: setShowSegmentationMask,
      hasContent: hasSegment,
    },
    {
      id: 'base',
      labelKey: 'layers.base',
      icon: 'RAW',
      visible: true,
      setVisible: () => {},
      hasContent: hasSource,
      alwaysEnabled: true,
    },
  ]

  return (
    <div className='flex flex-col'>
      {layers.map((layer) => (
        <LayerItem key={layer.id} layer={layer} />
      ))}
    </div>
  )
}

function LayerItem({ layer }: { layer: Layer }) {
  const { t } = useTranslation()
  const isLocked = layer.alwaysEnabled
  const canToggle = layer.hasContent && !isLocked
  const isActive = layer.hasContent && layer.visible

  return (
    <motion.div
      data-testid={`layer-${layer.id}`}
      data-has-content={layer.hasContent ? 'true' : 'false'}
      data-visible={layer.visible ? 'true' : 'false'}
      className={cn(
        'group flex items-center gap-2 px-2 py-1.5',
        !layer.hasContent && !isLocked && 'opacity-40',
      )}
      whileHover={{ backgroundColor: 'rgba(0,0,0,0.03)' }}
      transition={{ duration: 0.15 }}
    >
      {/* Visibility toggle */}
      <Button
        variant='ghost'
        size='icon-xs'
        onClick={(e) => {
          e.stopPropagation()
          if (canToggle) {
            layer.setVisible(!layer.visible)
          }
        }}
        disabled={!canToggle}
        className={cn('size-5', canToggle ? 'cursor-pointer' : 'cursor-default')}
      >
        {layer.visible ? (
          <EyeIcon
            className={cn('size-3.5', isActive ? 'text-foreground' : 'text-muted-foreground')}
          />
        ) : (
          <EyeOffIcon className='size-3.5 text-muted-foreground/40' />
        )}
      </Button>

      {/* Layer type indicator */}
      <div
        className={cn(
          'flex size-5 shrink-0 items-center justify-center rounded',
          !layer.hasContent && !isLocked ? 'text-muted-foreground/40' : 'text-muted-foreground',
        )}
      >
        {layer.icon === 'RAW' ? (
          <span className='text-[8px] font-bold'>RAW</span>
        ) : (
          <layer.icon className='size-3.5' />
        )}
      </div>

      {/* Layer name */}
      <span
        className={cn(
          'flex-1 truncate text-xs',
          !layer.hasContent && !isLocked
            ? 'text-muted-foreground/60'
            : isActive
              ? 'text-foreground'
              : 'text-muted-foreground',
        )}
      >
        {t(layer.labelKey)}
      </span>

      {/* Content indicator */}
      <div
        className={cn(
          'size-1.5 shrink-0 rounded-full',
          layer.hasContent ? 'bg-rose-500' : 'bg-muted-foreground/20',
        )}
      />
    </motion.div>
  )
}
