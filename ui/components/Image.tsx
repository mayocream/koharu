'use client'

import { useEffect, useState } from 'react'
import { convertToBlob } from '@/lib/util'

type ImageProps = {
  data?: number[]
  visible?: boolean
  opacity?: number
} & Omit<React.ImgHTMLAttributes<HTMLImageElement>, 'src'>

export function Image({
  data,
  visible = true,
  opacity = 1,
  style,
  alt = '',
  ...props
}: ImageProps) {
  const [src, setSrc] = useState<string | null>(null)

  useEffect(() => {
    if (!data) {
      setSrc(null)
      return
    }
    const blob = convertToBlob(data)
    const objectUrl = URL.createObjectURL(blob)
    setSrc(objectUrl)
    return () => {
      URL.revokeObjectURL(objectUrl)
    }
  }, [data])

  if (!visible || !src) return null

  return (
    <img
      {...props}
      alt={alt}
      src={src}
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
