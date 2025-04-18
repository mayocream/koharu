'use client'

import Konva from 'konva'
import { useEffect, useRef } from 'react'
import { useStageStore } from '@/lib/state'
import { storage } from '@/lib/storage'
import { debug } from '@tauri-apps/plugin-log'
import { initializeStageWithImage } from '@/lib/stage'
import ScaleControl from './scale-control'

function Canvas() {
  const containerRef = useRef<HTMLDivElement>(null)
  const stageRef = useRef(null)
  const { setStage } = useStageStore()

  const initializeStage = async () => {
    const serialized = await storage.get<string | null>('stage')
    let stage: Konva.Stage
    if (serialized) {
      stage = Konva.Node.create(serialized, stageRef.current)
      initializeStageWithImage(stage, stage.getAttr('image'))
      debug('Restored stage from storage')
    } else {
      stage = new Konva.Stage({
        container: stageRef.current,
        width: containerRef.current?.offsetWidth,
        height: containerRef.current?.offsetHeight,
      })
    }

    setStage(stage)
    debug(`Stage initialized`)
  }

  useEffect(() => {
    initializeStage()
  }, [stageRef])

  return (
    <div className='relative' ref={containerRef}>
      <div className='absolute min-w-full min-h-full flex items-center justify-center'>
        <div className='bg-white' ref={stageRef} />
      </div>
      <ScaleControl />
    </div>
  )
}

export default Canvas
