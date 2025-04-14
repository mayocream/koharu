import { Plus, Eye, Trash2 } from 'lucide-react'

function LayerPanel() {
  return (
    <div className='bg-white rounded-lg shadow-md w-72 border border-gray-200'>
      {/* Header */}
      <div className='flex items-center justify-between p-3 border-b border-gray-200'>
        <div className='flex items-center'>
          <h2 className='text-lg font-medium'>レイヤー</h2>
        </div>
      </div>

      {/* Toolbar */}
      <div className='flex items-center p-2 border-b border-gray-200'>
        <button className='p-1 mx-1 text-gray-700 hover:bg-gray-100 rounded'>
          <Plus size={20} />
        </button>

        <button className='p-1 mx-1 text-gray-700 hover:bg-gray-100 rounded ml-auto'>
          <Trash2 size={20} />
        </button>
      </div>

      {/* Layers List */}
      <div className='p-3'>
        {/* Text Layer */}
        <div className='flex items-center mb-2 p-2 hover:bg-gray-100 rounded'>
          <button className='mr-2 text-gray-700'>
            <Eye size={18} />
          </button>
          <div className='w-10 h-10 bg-gray-200 flex items-center justify-center rounded mr-2 text-gray-700 font-bold'>
            T
          </div>
          <span className='text-sm'>テキストを追加</span>
        </div>

        {/* Image Layer */}
        <div className='flex items-center p-2 bg-blue-50 border-2 border-blue-500 rounded'>
          <button className='mr-2 text-gray-700'>
            <Eye size={18} />
          </button>
          <div className='w-10 h-10 bg-white border border-gray-300 rounded mr-2 flex items-center justify-center overflow-hidden'>
            <svg viewBox='0 0 24 24' width='20' height='20' fill='#1DA1F2'>
              <path d='M22.46 6c-.77.35-1.6.58-2.46.69.88-.53 1.56-1.37 1.88-2.38-.83.5-1.75.85-2.72 1.05C18.37 4.5 17.26 4 16 4c-2.35 0-4.27 1.92-4.27 4.29 0 .34.04.67.11.98C8.28 9.09 5.11 7.38 3 4.79c-.37.63-.58 1.37-.58 2.15 0 1.49.75 2.81 1.91 3.56-.71 0-1.37-.2-1.95-.5v.03c0 2.08 1.48 3.82 3.44 4.21a4.22 4.22 0 0 1-1.93.07 4.28 4.28 0 0 0 4 2.98 8.521 8.521 0 0 1-5.33 1.84c-.34 0-.68-.02-1.02-.06C3.44 20.29 5.7 21 8.12 21 16 21 20.33 14.46 20.33 8.79c0-.19 0-.37-.01-.56.84-.6 1.56-1.36 2.14-2.23z' />
            </svg>
          </div>
          <span className='text-sm'>レイヤー1</span>
        </div>
      </div>
    </div>
  )
}

export default LayerPanel
