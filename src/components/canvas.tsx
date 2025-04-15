import Konva from 'konva'
import { useEffect, useRef } from 'react'
import { useStageStore } from '@/lib/state'

function Canvas() {
  const ref = useRef(null)
  const { setStage } = useStageStore()

  useEffect(() => {
    const stage = new Konva.Stage({
      container: ref.current,
      width: 200,
      height: 200,
    })

    setStage(stage)

    return () => {
      setStage(null)
      stage.destroy()
    }
  }, [])

  return <div className='bg-white' ref={ref} />
}

export default Canvas
