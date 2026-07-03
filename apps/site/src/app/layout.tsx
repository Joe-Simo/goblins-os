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
    "Goblins OS is an open AI-native desktop for building local software on Fedora bootc.",
  alternates: {
    canonical: "/",
  },
  icons: {
    icon: "/favicon.svg",
    shortcut: "/favicon.svg",
    apple: "/favicon.svg",
  },
  openGraph: {
    title: "Goblins OS",
    description:
      "An open AI-native desktop for building local software on Fedora bootc.",
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
      "An open AI-native desktop for building local software on Fedora bootc.",
    images: ["/screenshots/home.png"],
  },
};

export const viewport: Viewport = {
  width: "device-width",
  initialScale: 1,
  colorScheme: "light",
  themeColor: "#ffffff",
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
