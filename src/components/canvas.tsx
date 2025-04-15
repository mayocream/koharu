import { Stage } from 'react-konva'
import { useStage } from './stage-provider'

function Canvas() {
  const stageRef = useStage()

  return <Stage className='bg-white' ref={stageRef} />
}

export default Canvas
