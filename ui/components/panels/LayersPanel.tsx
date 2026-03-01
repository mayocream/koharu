'use client'

import { useTranslation } from 'react-i18next'
import { motion } from 'motion/react'
import {
  EyeIcon,
  EyeOffIcon,
  SparklesIcon,
  ALargeSmallIcon,
  ContrastIcon,
  BandageIcon,
  PaintbrushIcon,
} from 'lucide-react'
import { cn } from '@/lib/utils'
import { Button } from '@/components/ui/button'
import { useEditorUiStore } from '@/lib/stores/editorUiStore'
import { useCurrentDocumentState } from '@/lib/query/hooks'

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
  const { currentDocument } = useCurrentDocumentState()
  const showInpaintedImage = useEditorUiStore(
    (state) => state.showInpaintedImage,
  )
  const setShowInpaintedImage = useEditorUiStore(
    (state) => state.setShowInpaintedImage,
  )
  const showSegmentationMask = useEditorUiStore(
    (state) => state.showSegmentationMask,
  )
  const setShowSegmentationMask = useEditorUiStore(
    (state) => state.setShowSegmentationMask,
  )
  const showBrushLayer = useEditorUiStore((state) => state.showBrushLayer)
  const setShowBrushLayer = useEditorUiStore((state) => state.setShowBrushLayer)
  const showTextBlocksOverlay = useEditorUiStore(
    (state) => state.showTextBlocksOverlay,
  )
  const setShowTextBlocksOverlay = useEditorUiStore(
    (state) => state.setShowTextBlocksOverlay,
  )
  const showRenderedImage = useEditorUiStore((state) => state.showRenderedImage)
  const setShowRenderedImage = useEditorUiStore(
    (state) => state.setShowRenderedImage,
  )

  const layers: Layer[] = [
    {
      id: 'rendered',
      labelKey: 'layers.rendered',
      icon: SparklesIcon,
      visible: showRenderedImage,
      setVisible: setShowRenderedImage,
      hasContent: currentDocument?.rendered !== undefined,
    },
    {
      id: 'textBlocks',
      labelKey: 'layers.textBlocks',
      icon: ALargeSmallIcon,
      visible: showTextBlocksOverlay,
      setVisible: setShowTextBlocksOverlay,
      hasContent:
        currentDocument?.textBlocks !== undefined &&
        currentDocument.textBlocks.length > 0,
    },
    {
      id: 'brush',
      labelKey: 'layers.brush',
      icon: PaintbrushIcon,
      visible: showBrushLayer,
      setVisible: setShowBrushLayer,
      hasContent: currentDocument?.brushLayer !== undefined,
    },
    {
      id: 'inpainted',
      labelKey: 'layers.inpainted',
      icon: BandageIcon,
      visible: showInpaintedImage,
      setVisible: setShowInpaintedImage,
      hasContent: currentDocument?.inpainted !== undefined,
    },
    {
      id: 'mask',
      labelKey: 'layers.mask',
      icon: ContrastIcon,
      visible: showSegmentationMask,
      setVisible: setShowSegmentationMask,
      hasContent: currentDocument?.segment !== undefined,
    },
    {
      id: 'base',
      labelKey: 'layers.base',
      icon: 'RAW',
      visible: true,
      setVisible: () => {},
      hasContent: currentDocument?.image !== undefined,
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
        className={cn(
          'size-5',
          canToggle ? 'cursor-pointer' : 'cursor-default',
        )}
      >
        {layer.visible ? (
          <EyeIcon
            className={cn(
              'size-3.5',
              isActive ? 'text-foreground' : 'text-muted-foreground',
            )}
          />
        ) : (
          <EyeOffIcon className='text-muted-foreground/40 size-3.5' />
        )}
      </Button>

      {/* Layer type indicator */}
      <div
        className={cn(
          'flex size-5 shrink-0 items-center justify-center rounded',
          !layer.hasContent && !isLocked
            ? 'text-muted-foreground/40'
            : 'text-muted-foreground',
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
