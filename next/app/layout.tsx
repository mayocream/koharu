import type { Metadata } from 'next'
import { Noto_Sans_JP, Noto_Sans_SC } from 'next/font/google'
import './globals.css'

const notoSansJP = Noto_Sans_JP({ subsets: ['latin'] })
const notoSansSC = Noto_Sans_SC({ subsets: ['latin'] })

export const metadata: Metadata = {
  title: 'Koharu',
}

export default function RootLayout({
  children,
}: Readonly<{
  children: React.ReactNode
}>) {
  return (
    <html lang='ja' suppressHydrationWarning>
      <body
        className={`${notoSansJP.className} ${notoSansSC.className} antialiased`}
      >
        {children}
      </body>
    </html>
  )
}
