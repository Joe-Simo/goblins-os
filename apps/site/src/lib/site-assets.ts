export type ScreenshotAsset = {
  src: string;
  alt: string;
  title: string;
  description: string;
  width: number;
  height: number;
  bytes: number;
};

export const screenshots = [
  {
    src: "/screenshots/home.png",
    alt: "Goblins OS home screen asking what the user wants to make",
    title: "Home",
    description: "Describe an app and keep the build local.",
    width: 1400,
    height: 590,
    bytes: 102004,
  },
  {
    src: "/screenshots/build-studio.png",
    alt: "Goblins OS Build Studio in a live desktop session",
    title: "Build Studio",
    description: "Threads, changed files, and build output in one native surface.",
    width: 1400,
    height: 590,
    bytes: 592982,
  },
  {
    src: "/screenshots/mission-control.png",
    alt: "Goblins OS Mission Control with spaces and windows",
    title: "Mission Control",
    description: "Window switching, spaces, and the adaptive desktop shell.",
    width: 1400,
    height: 875,
    bytes: 360000,
  },
  {
    src: "/screenshots/installer-disk.png",
    alt: "Goblins OS installer disk selection screen",
    title: "Installer",
    description: "Architecture-specific media with guarded storage choices.",
    width: 1400,
    height: 590,
    bytes: 56843,
  },
] satisfies ScreenshotAsset[];

export const assetBudget = {
  screenshotBytes: screenshots.reduce((total, screenshot) => total + screenshot.bytes, 0),
  demoVideoBytes: 432314,
  maxInitialScreenshotBytes: 450000,
  isoPrefetch: false,
  autoplayLargeVideoOnMobile: false,
};
