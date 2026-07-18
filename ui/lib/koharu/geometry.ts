import type { Frame, HitTarget } from './protocol'

export function transformFrame(frame: Frame, target: HitTarget, dx: number, dy: number): Frame {
  if (target.type === 'element') return { ...frame, x: frame.x + dx, y: frame.y + dy }

  const angle = (frame.angle_degrees * Math.PI) / 180
  const cos = Math.cos(angle)
  const sin = Math.sin(angle)
  const localDx = dx * cos + dy * sin
  const localDy = -dx * sin + dy * cos
  let left = -frame.width / 2
  let right = frame.width / 2
  let top = -frame.height / 2
  let bottom = frame.height / 2

  if (target.handle.includes('west')) left += localDx
  if (target.handle.includes('east')) right += localDx
  if (target.handle.includes('north')) top += localDy
  if (target.handle.includes('south')) bottom += localDy

  if (right - left < 1) {
    if (target.handle.includes('west')) left = right - 1
    else right = left + 1
  }
  if (bottom - top < 1) {
    if (target.handle.includes('north')) top = bottom - 1
    else bottom = top + 1
  }

  const localCenterX = (left + right) / 2
  const localCenterY = (top + bottom) / 2
  const centerOffsetX = localCenterX * cos - localCenterY * sin
  const centerOffsetY = localCenterX * sin + localCenterY * cos
  const width = right - left
  const height = bottom - top
  const centerX = frame.x + frame.width / 2 + centerOffsetX
  const centerY = frame.y + frame.height / 2 + centerOffsetY
  return {
    ...frame,
    x: centerX - width / 2,
    y: centerY - height / 2,
    width,
    height,
  }
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
