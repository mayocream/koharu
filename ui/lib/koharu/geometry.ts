import type { EntityView, Frame, TransformFrame } from './protocol'

const MIN_FRAME_SIZE = 1e-6

export interface CssFrame {
  left: number
  top: number
  width: number
  height: number
  angle: number
}

export function draftFrame(start: [number, number], end: [number, number]): Frame {
  return {
    x: Math.min(start[0], end[0]),
    y: Math.min(start[1], end[1]),
    width: Math.max(1, Math.abs(end[0] - start[0])),
    height: Math.max(1, Math.abs(end[1] - start[1])),
    angle_degrees: 0,
  }
}

export function pagePoint(
  clientX: number,
  clientY: number,
  bounds: DOMRect,
  camera: { zoom: number; translation: [number, number] },
  dpr = window.devicePixelRatio,
): [number, number] {
  const x = (clientX - bounds.x) * dpr
  const y = (clientY - bounds.y) * dpr
  return [(x - camera.translation[0]) / camera.zoom, (y - camera.translation[1]) / camera.zoom]
}

export function zoomAtPoint(
  camera: { zoom: number; translation: [number, number] },
  point: [number, number],
  nextZoom: number,
): { zoom: number; translation: [number, number] } {
  const pageX = (point[0] - camera.translation[0]) / camera.zoom
  const pageY = (point[1] - camera.translation[1]) / camera.zoom
  return {
    zoom: nextZoom,
    translation: [point[0] - pageX * nextZoom, point[1] - pageY * nextZoom],
  }
}

export function scrollCamera(
  camera: { zoom: number; translation: [number, number] },
  delta: [number, number],
  dpr = window.devicePixelRatio,
): { zoom: number; translation: [number, number] } {
  return {
    zoom: camera.zoom,
    translation: [
      camera.translation[0] - delta[0] * dpr,
      camera.translation[1] - delta[1] * dpr,
    ],
  }
}

/** Mirrors the frame projection used by the Vello canvas for React hit testing. */
export function entityFrame(entity: EntityView): Frame | null {
  const points = entity.geometry?.points
  if (!points?.length || points.some((point) => !finite(point.x, point.y))) return null
  if (points.length === 4) {
    const [topLeft, topRight, bottomRight, bottomLeft] = points
    const top: [number, number] = [topRight.x - topLeft.x, topRight.y - topLeft.y]
    const right: [number, number] = [bottomRight.x - topRight.x, bottomRight.y - topRight.y]
    const bottom: [number, number] = [bottomLeft.x - bottomRight.x, bottomLeft.y - bottomRight.y]
    const left: [number, number] = [topLeft.x - bottomLeft.x, topLeft.y - bottomLeft.y]
    const width = Math.hypot(...top)
    const height = Math.hypot(...right)
    if (width > MIN_FRAME_SIZE && height > MIN_FRAME_SIZE) {
      const scale = Math.max(width, height, 1)
      const oppositeLengthsMatch =
        Math.abs(Math.hypot(...bottom) - width) <= scale * 1e-6 &&
        Math.abs(Math.hypot(...left) - height) <= scale * 1e-6
      const perpendicular = Math.abs(top[0] * right[0] + top[1] * right[1]) <= width * height * 1e-6
      const diagonalsBisect =
        Math.abs(topLeft.x + bottomRight.x - topRight.x - bottomLeft.x) <= scale * 1e-6 &&
        Math.abs(topLeft.y + bottomRight.y - topRight.y - bottomLeft.y) <= scale * 1e-6
      if (oppositeLengthsMatch && perpendicular && diagonalsBisect) {
        const centerX = points.reduce((sum, point) => sum + point.x, 0) * 0.25
        const centerY = points.reduce((sum, point) => sum + point.y, 0) * 0.25
        return {
          x: centerX - width * 0.5,
          y: centerY - height * 0.5,
          width,
          height,
          angle_degrees: (Math.atan2(top[1], top[0]) * 180) / Math.PI,
        }
      }
    }
  }

  const xs = points.map((point) => point.x)
  const ys = points.map((point) => point.y)
  const minX = Math.min(...xs)
  const minY = Math.min(...ys)
  const width = Math.max(...xs) - minX
  const height = Math.max(...ys) - minY
  return width > MIN_FRAME_SIZE && height > MIN_FRAME_SIZE
    ? { x: minX, y: minY, width, height, angle_degrees: 0 }
    : null
}

export function hitTestEntities(
  entities: EntityView[],
  point: [number, number],
  selectable: (entity: EntityView) => boolean,
): EntityView | null {
  for (let index = entities.length - 1; index >= 0; index -= 1) {
    const entity = entities[index]
    if (!selectable(entity) || !entity.visibility.visible || entity.visibility.opacity <= 0) continue
    const frame = entityFrame(entity)
    if (frame && frameContains(frame, point)) return entity
  }
  return null
}

export function frameContains(frame: Frame, point: [number, number]): boolean {
  const centerX = frame.x + frame.width * 0.5
  const centerY = frame.y + frame.height * 0.5
  const angle = (-frame.angle_degrees * Math.PI) / 180
  const cos = Math.cos(angle)
  const sin = Math.sin(angle)
  const dx = point[0] - centerX
  const dy = point[1] - centerY
  const localX = dx * cos - dy * sin
  const localY = dx * sin + dy * cos
  return Math.abs(localX) <= frame.width * 0.5 && Math.abs(localY) <= frame.height * 0.5
}

export function cssFrame(
  frame: Frame,
  camera: { zoom: number; translation: [number, number] },
  dpr = window.devicePixelRatio,
): CssFrame {
  const scale = camera.zoom / dpr
  return {
    left: (frame.x * camera.zoom + camera.translation[0]) / dpr,
    top: (frame.y * camera.zoom + camera.translation[1]) / dpr,
    width: frame.width * scale,
    height: frame.height * scale,
    angle: frame.angle_degrees,
  }
}

export function translateFrames(
  originals: TransformFrame[],
  delta: [number, number],
): TransformFrame[] {
  return originals.map(({ element, frame }) => ({
    element,
    frame: { ...frame, x: frame.x + delta[0], y: frame.y + delta[1] },
  }))
}

function finite(...values: number[]): boolean {
  return values.every(Number.isFinite)
}
