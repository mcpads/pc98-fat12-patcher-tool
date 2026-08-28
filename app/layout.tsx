import type { Metadata } from 'next';
import './globals.css';

export const metadata: Metadata = {
  title: 'PC-98 FAT12 한글 패처',
  description:
    '사용자가 가진 원본 HDM에서 기준 디스크를 재구성하고 BPS를 적용하는 로컬 웹 패처',
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
