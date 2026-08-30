import type { Metadata } from 'next';
import './globals.css';

export const metadata: Metadata = {
  title: '레트로 게임 한글 패처 | Retro Patcher',
  description: '원본 디스크와 CD 이미지를 브라우저 안에서 검사하고 패치 ZIP을 적용하는 로컬 웹 패처',
  alternates: {
    canonical: 'https://patcher.retrogame.cloud/',
  },
  openGraph: {
    type: 'website',
    locale: 'ko_KR',
    url: 'https://patcher.retrogame.cloud/',
    siteName: 'Retro Patcher',
    title: '레트로 게임 한글 패처 | Retro Patcher',
    description: '원본 디스크와 CD 이미지를 브라우저 안에서 검사하고 패치 ZIP을 적용하는 로컬 웹 패처',
  },
};

export default function RootLayout({
  children,
}: Readonly<{
  children: React.ReactNode;
}>) {
  return (
    <html lang="ko">
      <body>{children}</body>
    </html>
  );
}
