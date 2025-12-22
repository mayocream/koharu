'use client'

import type { CSSProperties } from 'react'
import { useCallback, useEffect, useRef, useState } from 'react'
import { convertToBlob } from '@/lib/util'

type ImageProps = {
  data?: number[]
  visible?: boolean
  opacity?: number
  transition?: boolean
  dataKey?: string | number
} & Omit<React.ImgHTMLAttributes<HTMLImageElement>, 'src'>

const FADE_DURATION_MS = 180

// Cross-fade between successive image buffers to avoid UI flicker when
// swapping inpaint results.
export function Image({
  data,
  visible = true,
  opacity = 1,
  transition = true,
  dataKey,
  style,
  alt = '',
  ...props
}: ImageProps) {
  const dataDep = dataKey ?? data

  // Simple path without transitions (used for static base image to avoid extra paints)
  const [plainSrc, setPlainSrc] = useState<string | null>(null)
  useEffect(() => {
    if (!transition) {
      if (!dataDep || !data) {
        setPlainSrc(null)
        return
      }
      const blob = convertToBlob(data)
      const url = URL.createObjectURL(blob)
      setPlainSrc(url)
      return () => URL.revokeObjectURL(url)
    }
    setPlainSrc(null)
    return
  }, [data, dataDep, transition])

  if (!transition) {
    if (!visible || !plainSrc) return null
    return (
      <img
        {...props}
        alt={alt}
        src={plainSrc}
        draggable={false}
        style={{
          position: 'absolute',
          inset: 0,
          pointerEvents: 'none',
          userSelect: 'none',
          width: '100%',
          height: '100%',
          objectFit: 'contain',
          ...style,
          opacity,
        }}
      />
    )
  }

  const [currentSrc, setCurrentSrc] = useState<string | null>(null)
  const [nextSrc, setNextSrc] = useState<string | null>(null)
  const [crossfade, setCrossfade] = useState(false)

  const currentSrcRef = useRef<string | null>(null)
  const nextSrcRef = useRef<string | null>(null)

  const cleanupUrl = useCallback((url: string | null) => {
    if (url) URL.revokeObjectURL(url)
  }, [])

  useEffect(() => {
    currentSrcRef.current = currentSrc
  }, [currentSrc])

  useEffect(() => {
    nextSrcRef.current = nextSrc
  }, [nextSrc])

  useEffect(() => {
    return () => {
      cleanupUrl(currentSrcRef.current)
      cleanupUrl(nextSrcRef.current)
    }
  }, [cleanupUrl])

  const promoteNext = useCallback(() => {
    const incoming = nextSrcRef.current
    if (!incoming) return
    const outgoing = currentSrcRef.current

    currentSrcRef.current = incoming
    setCurrentSrc(incoming)
    setNextSrc(null)
    setCrossfade(false)

    if (outgoing && outgoing !== incoming) {
      cleanupUrl(outgoing)
    }
  }, [cleanupUrl])

  useEffect(() => {
    if (!dataDep || !data) {
      cleanupUrl(currentSrcRef.current)
      cleanupUrl(nextSrcRef.current)
      currentSrcRef.current = null
      nextSrcRef.current = null
      setCurrentSrc(null)
      setNextSrc(null)
      setCrossfade(false)
      return
    }

    const blob = convertToBlob(data)
    const objectUrl = URL.createObjectURL(blob)
    let cancelled = false

    const preload = new window.Image()
    preload.onload = () => {
      if (cancelled) {
        cleanupUrl(objectUrl)
        return
      }

      // First image, render immediately
      if (!currentSrcRef.current) {
        currentSrcRef.current = objectUrl
        setCurrentSrc(objectUrl)
        return
      }

      // Subsequent images: queue and cross-fade
      setNextSrc((prev) => {
        if (prev && prev !== currentSrcRef.current) {
          cleanupUrl(prev)
        }
        return objectUrl
      })

      setCrossfade(false)
      requestAnimationFrame(() => {
        requestAnimationFrame(() => setCrossfade(true))
      })
    }

    preload.src = objectUrl

    return () => {
      cancelled = true
      if (
        objectUrl !== currentSrcRef.current &&
        objectUrl !== nextSrcRef.current
      ) {
        cleanupUrl(objectUrl)
      }
    }
  }, [data, dataDep, cleanupUrl])

  useEffect(() => {
    if (!nextSrc || !crossfade) return
    const timeout = window.setTimeout(
      promoteNext,
      FADE_DURATION_MS + 50, // safety fallback in case transitionend doesn't fire
    )
    return () => window.clearTimeout(timeout)
  }, [nextSrc, crossfade, promoteNext])

  if (!visible || (!currentSrc && !nextSrc)) return null

  const baseStyle: CSSProperties = {
    position: 'absolute',
    inset: 0,
    pointerEvents: 'none',
    userSelect: 'none',
    width: '100%',
    height: '100%',
    objectFit: 'contain',
    ...style,
  }

  return (
    <>
      {currentSrc && (
        <img
          {...props}
          alt={alt}
          src={currentSrc}
          draggable={false}
          style={{
            ...baseStyle,
            opacity: nextSrc ? (crossfade ? 0 : opacity) : opacity,
            transition:
              nextSrc && crossfade
                ? `opacity ${FADE_DURATION_MS}ms ease`
                : undefined,
          }}
        />
      )}
      {nextSrc && (
        <img
          {...props}
          alt={alt}
          src={nextSrc}
          draggable={false}
          onTransitionEnd={promoteNext}
          style={{
            ...baseStyle,
            opacity: crossfade ? opacity : 0,
            transition: `opacity ${FADE_DURATION_MS}ms ease`,
          }}
        />
      )}
    </>
  )
}
