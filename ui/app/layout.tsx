import type { Metadata } from 'next'
import { Noto_Sans_JP } from 'next/font/google'
import './globals.css'
import Providers from '@/app/providers'

const notoSansJP = Noto_Sans_JP({
  subsets: ['latin'],
})

export const metadata: Metadata = {
  title: 'Koharu',
}

function RootLayout({
  children,
}: Readonly<{
  children: React.ReactNode
}>) {
  return (
    <html lang='en-US'>
      <body
        className={`${notoSansJP.className} antialiased`}
        suppressHydrationWarning
      >
        <Providers>{children}</Providers>
      </body>
    </html>
  )
}

export default RootLayout
