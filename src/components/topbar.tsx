import { open } from '@tauri-apps/plugin-dialog'
import { Save, Image } from 'lucide-react'
import { debug } from '@tauri-apps/plugin-log'
import Konva from 'konva'
import { convertFileSrc } from '@tauri-apps/api/core'
import { useStageStore } from '@/lib/state'

function Topbar() {
  const { stage } = useStageStore()
  const handleOpenFile = async () => {
    const selected = await open({
      multiple: false,
      filters: [
        {
          name: 'Image',
          extensions: ['png', 'jpeg', 'jpg'],
        },
      ],
    })

    debug(`Opened file: ${selected}`)

    Konva.Image.fromURL(
      convertFileSrc(selected),
      (img) => {
        let width = img.width()
        let height = img.height()

        stage.destroyChildren()
        stage.width(width)
        stage.height(height)
        img.setAttrs({
          x: 0,
          y: 0,
          width,
          height,
        })

        const layer = new Konva.Layer()
        layer.add(img)
        stage.add(layer)
      },
      (err) => {
        alert(`Error loading image: ${err}`)
      }
    )
  }

  return (
    <div className='flex items-center p-2 bg-white border-b border-gray-200 shadow-sm'>
      <div className='flex items-center'>
        <button
          className='flex items-center p-2 mx-1 text-gray-700 hover:bg-gray-100 rounded'
          onClick={handleOpenFile}
        >
          <Image size={18} />
          <span className='ml-1'>ファイルを開く</span>
        </button>

        <button className='flex items-center p-2 mx-1 text-gray-700 hover:bg-gray-100 rounded'>
          <Save size={18} />
          <span className='ml-1'>保存</span>
        </button>
      </div>
    </div>
  )
}

export default Topbar
