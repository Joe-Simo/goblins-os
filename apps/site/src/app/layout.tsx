import type { Metadata, Viewport } from "next";
import { TooltipProvider } from "@/components/ui/tooltip";
import "./globals.css";

export const metadata: Metadata = {
  metadataBase: new URL("https://goblinsos.com"),
  title: {
    default: "Goblins OS — Imagine it. Build it. Keep it.",
    template: "%s | Goblins OS",
  },
  description:
    "Meet Goblins OS, the open Linux desktop that helps turn your ideas into working software—right on your machine.",
  applicationName: "Goblins OS",
  category: "technology",
  keywords: [
    "Goblins OS",
    "Linux desktop",
    "AI-native operating system",
    "Build Studio",
    "open source",
    "ARM64 virtual machine",
  ],
  alternates: {
    canonical: "/",
  },
  icons: {
    icon: "/favicon.svg",
    shortcut: "/favicon.svg",
  },
  openGraph: {
    title: "Goblins OS",
    description:
      "The open Linux desktop that helps turn your ideas into working software—right on your machine.",
    url: "https://goblinsos.com",
    siteName: "Goblins OS",
    images: [
      {
        url: "/screenshots/build-studio.png",
        width: 1400,
        height: 590,
        alt: "Goblins OS Build Studio on the native desktop",
      },
    ],
    locale: "en_US",
    type: "website",
  },
  twitter: {
    card: "summary_large_image",
    title: "Goblins OS",
    description:
      "The open Linux desktop that helps turn your ideas into working software—right on your machine.",
    images: ["/screenshots/build-studio.png"],
  },
  robots: {
    index: true,
    follow: true,
  },
};

export const viewport: Viewport = {
  width: "device-width",
  initialScale: 1,
  colorScheme: "light",
  themeColor: "#f3f0e6",
};

export default function RootLayout({
  children,
}: Readonly<{
  children: React.ReactNode;
}>) {
  return (
    <html lang="en">
      <body>
        <TooltipProvider>{children}</TooltipProvider>
      </body>
    </html>
  );
}
