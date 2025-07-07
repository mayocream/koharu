import type { Metadata } from 'next'
import { Theme } from '@radix-ui/themes'
import { Noto_Sans_JP, Noto_Sans_SC } from 'next/font/google'
import './globals.css'

const notoSansJP = Noto_Sans_JP({ subsets: ['latin'] })
const notoSansSC = Noto_Sans_SC({ subsets: ['latin'] })

export const metadata: Metadata = {
  title: 'Koharu',
  description: 'Manga Translator',
}

export default function RootLayout({
  children,
}: Readonly<{
  children: React.ReactNode
}>) {
  return (
    <html lang='en'>
      <body
        className={`${notoSansJP.className} ${notoSansSC.className} antialiased`}
        suppressHydrationWarning
      >
        <Theme accentColor='ruby'>{children}</Theme>
      </body>
    </html>
  )
}
