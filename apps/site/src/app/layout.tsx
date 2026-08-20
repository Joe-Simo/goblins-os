import type { Metadata, Viewport } from "next";
import { TooltipProvider } from "@/components/ui/tooltip";
import "./globals.css";

export const metadata: Metadata = {
  metadataBase: new URL("https://goblinsos.com"),
  title: {
    default: "Goblins OS — Build locally. Keep control.",
    template: "%s | Goblins OS",
  },
  description:
    "Download the Goblins OS public aarch64 preview—an open AI-native Linux desktop for building local software.",
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
      "The public aarch64 preview of an open AI-native Linux desktop for building local software.",
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
      "The public aarch64 preview of an open AI-native Linux desktop for building local software.",
    images: ["/screenshots/build-studio.png"],
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
