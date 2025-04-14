import { Layer, Stage, Text } from 'react-konva'
import LayerPanel from './components/layer-panel'
import Tools from './components/tools'
import Topbar from './components/topbar'

function App() {
  return (
    <main className='flex flex-col h-screen w-screen bg-gray-100'>
      <Topbar />
      <div className='flex flex-1 my-2'>
        <Tools />

        <div className='flex-1 flex items-center justify-center'>
          <Stage width={200} height={200} className='bg-white'>
            <Layer>
              <Text text='Hello, World!' fontSize={30} fill='black' />
            </Layer>
          </Stage>
        </div>

        <div className='h-full'>
          <LayerPanel />
        </div>
      </div>
    </main>
  )
}

export default App
