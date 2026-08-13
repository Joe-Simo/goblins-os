import type { Metadata, Viewport } from "next";
import { TooltipProvider } from "@/components/ui/tooltip";
import "./globals.css";

export const metadata: Metadata = {
  metadataBase: new URL("https://goblinsos.com"),
  title: {
    default: "Goblins OS",
    template: "%s | Goblins OS",
  },
  description:
    "Goblins OS is an open AI-native Linux desktop for building local software.",
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
      "An open AI-native Linux desktop for building local software.",
    url: "https://goblinsos.com",
    siteName: "Goblins OS",
    images: [
      {
        url: "/screenshots/home.png",
        width: 1400,
        height: 591,
        alt: "Goblins OS home screen",
      },
    ],
    locale: "en_US",
    type: "website",
  },
  twitter: {
    card: "summary_large_image",
    title: "Goblins OS",
    description:
      "An open AI-native Linux desktop for building local software.",
    images: ["/screenshots/home.png"],
  },
};

export const viewport: Viewport = {
  width: "device-width",
  initialScale: 1,
  colorScheme: "light dark",
  themeColor: [
    { media: "(prefers-color-scheme: light)", color: "#ffffff" },
    { media: "(prefers-color-scheme: dark)", color: "#18181b" },
  ],
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
