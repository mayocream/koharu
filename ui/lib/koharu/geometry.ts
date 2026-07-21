import type { Frame } from './protocol'

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
