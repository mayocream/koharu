import domtoimage from 'dom-to-image'
import { domRenderEnabled } from '@/lib/featureFlags'

export const DOM_LAYER_SELECTOR = '[data-dom-render-layer]'

export const waitForAnimationFrame = () =>
  new Promise<void>((resolve) => requestAnimationFrame(() => resolve()))

export const rasterizeDomLayer = async (
  scale: number,
  targetWidth: number,
  targetHeight: number,
): Promise<number[] | null> => {
  if (!domRenderEnabled) return null
  const layer = document.querySelector<HTMLElement>(DOM_LAYER_SELECTOR)
  if (!layer) return null
  const displayWidth = layer.clientWidth
  const displayHeight = layer.clientHeight
  if (!displayWidth || !displayHeight) return null

  const scaleRatio = Math.max(0.01, scale / 100)
  const inverseScale = 1 / scaleRatio
  const width = Math.round(targetWidth)
  const height = Math.round(targetHeight)
  const dataUrl = await domtoimage.toPng(layer, {
    width,
    height,
    bgcolor: 'transparent',
    cacheBust: true,
    style: {
      transform: `scale(${inverseScale})`,
      transformOrigin: 'top left',
      width: `${displayWidth}px`,
      height: `${displayHeight}px`,
    },
  })
  const buffer = await (await fetch(dataUrl)).arrayBuffer()
  return Array.from(new Uint8Array(buffer))
}
