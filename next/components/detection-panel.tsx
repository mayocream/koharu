import { useStageStore } from '@/lib/state'
import { Play } from 'lucide-react'
import { invoke } from '@tauri-apps/api/core'

function DetectionPanel() {
  const { stage } = useStageStore()

  const inference = async () => {
    const image = stage?.findOne('#image')
    if (!image) return

    const buffer = (await stage.toBlob()) as Blob

    const result = await invoke('detect', {
      image: await buffer.arrayBuffer(),
    })

    console.log('Detection result:', result)
  }

  return (
    <div className='bg-white rounded-lg shadow-md w-72 border border-gray-200'>
      {/* Header */}
      <div className='flex items-center p-3'>
        <h2 className='text-lg font-medium'>吹き出し</h2>
        <div className='flex-grow'></div>
        <button
          className='text-gray-500 hover:text-gray-700 hover:bg-gray-100 rounded-full p-2'
          onClick={inference}
        >
          <Play className='w-4 h-4' />
        </button>
      </div>
    </div>
  )
}

export default DetectionPanel
