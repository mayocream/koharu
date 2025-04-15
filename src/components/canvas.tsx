import Konva from 'konva'
import { useEffect, useRef } from 'react'
import { useStageStore } from '@/lib/state'
import { storage } from '@/lib/storage'
import { debug } from '@tauri-apps/plugin-log'
import { loadImage } from '@/lib/stage'

function Canvas() {
  const ref = useRef(null)
  const { setStage } = useStageStore()

  const initializeStage = async () => {
    const serialized = await storage.get<string | null>('stage')
    let stage: Konva.Stage
    if (serialized) {
      stage = Konva.Node.create(serialized, ref.current)
      loadImage(stage.getAttr('image'))
      debug('Restored stage from storage')
    } else {
      stage = new Konva.Stage({
        container: ref.current,
        width: 200,
        height: 200,
      })
    }

    setStage(stage)
  }

  useEffect(() => {
    initializeStage()

    return () => {
      setStage(null)
    }
  }, [ref])

  return <div className='bg-white' ref={ref} />
}

export default Canvas
