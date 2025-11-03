'use client'

import { useEffect, useState } from 'react'
import { Image as KonvaImage } from 'react-konva'
import { convertToImageBitmap } from '@/lib/util'

export function Image({
  data,
  ...props
}: { data?: number[] } & Omit<
  React.ComponentProps<typeof KonvaImage>,
  'image'
>) {
  const [imageBitmap, setImageBitmap] = useState<ImageBitmap | null>(null)

  useEffect(() => {
    if (data) convertToImageBitmap(data).then(setImageBitmap)
  }, [data])

  return imageBitmap ? <KonvaImage {...props} image={imageBitmap} /> : null
}
