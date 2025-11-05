import type { Metadata } from 'next'
import { Noto_Sans_JP } from 'next/font/google'
import './globals.css'
import { Tooltip } from 'radix-ui'

const notoSansJP = Noto_Sans_JP({
  subsets: ['latin'],
})

export const metadata: Metadata = {
  title: 'Koharu',
}

export default function RootLayout({
  children,
}: Readonly<{
  children: React.ReactNode
}>) {
  return (
    <html lang='en'>
      <body className={`${notoSansJP.className} antialiased`}>
        <Tooltip.Provider delayDuration={300}>{children}</Tooltip.Provider>
      </body>
    </html>
  )
}
