import { useCanvasStore } from '@/lib/state'
import { Play } from 'lucide-react'
import { invoke } from '@tauri-apps/api/core'

function DetectionPanel() {
  const { imageSrc, blocks, setBlocks } = useCanvasStore()
  const inference = async () => {
    const buffer = await fetch(imageSrc).then((res) => res.bytes())
    const result = await invoke<any>('detect', {
      image: buffer,
    })

    setBlocks(result.bboxes)
  }

  return (
    <div className='bg-white rounded-lg shadow-md w-72 border border-gray-200'>
      {/* Header */}
      <div className='flex items-center p-3'>
        <h2 className='text-lg font-medium'>吹き出し</h2>
        <div className='flex-grow'></div>
        <button
          className='text-gray-500 hover:text-gray-700 hover:bg-gray-100 rounded-full p-2 cursor-pointer'
          onClick={inference}
        >
          <Play className='w-4 h-4' />
        </button>
      </div>
    </div>
  )
}

export default DetectionPanel
