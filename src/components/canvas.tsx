import { Stage } from 'react-konva'
import { useStage } from './stage-provider'

function Canvas() {
  const stageRef = useStage()

  return <Stage width={200} height={200} className='bg-white' ref={stageRef} />
}

export default Canvas
