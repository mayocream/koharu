'use client'

export default function GlobalError({
  error,
  reset,
}: {
  error: Error & { digest?: string }
  reset: () => void
}) {
  return (
    <html lang='en'>
      <body className='grid min-h-screen place-items-center bg-background text-foreground'>
        <main className='max-w-lg rounded-lg border bg-card p-6 text-center shadow-lg'>
          <h1 className='text-lg font-semibold'>Koharu could not continue</h1>
          <p className='mt-2 text-sm text-muted-foreground'>{error.message}</p>
          <button
            className='mt-4 rounded-md bg-primary px-4 py-2 text-sm text-primary-foreground'
            onClick={reset}
          >
            Try again
          </button>
        </main>
      </body>
    </html>
  )
}
