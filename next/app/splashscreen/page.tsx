export default function SplashScreen() {
  return (
    <div className='flex h-screen w-screen flex-col items-center justify-center bg-gray-100'>
      <div className='flex h-16 w-16 animate-spin items-center justify-center rounded-full border-6 border-blue-400 border-t-transparent' />
      <h1 className='mt-4 text-2xl font-bold text-gray-800'>Initializing...</h1>
      <p className='mt-2 text-gray-600'>This may take a few seconds.</p>
    </div>
  )
}
