"use client";

import dynamic from "next/dynamic";
import { useEffect, useState } from "react";

const ThreeDeviceScene = dynamic(
  () => import("@/components/three-device-scene").then((mod) => mod.ThreeDeviceScene),
  {
    ssr: false,
    loading: () => <DeviceFallback />,
  },
);

function DeviceFallback() {
  return (
    <div
      aria-hidden="true"
      className="absolute inset-0 rounded-lg border border-border bg-[linear-gradient(140deg,#f8fafa,#eef6f2)]"
    />
  );
}

export function DevicePreview() {
  const [enabled, setEnabled] = useState(false);

  useEffect(() => {
    const frame = window.requestAnimationFrame(() => {
      const reduced = window.matchMedia("(prefers-reduced-motion: reduce)").matches;
      const mobile = window.matchMedia("(max-width: 767px)").matches;
      setEnabled(!reduced && !mobile);
    });

    return () => window.cancelAnimationFrame(frame);
  }, []);

  return (
    <div className="pointer-events-none absolute right-3 top-3 z-10 hidden size-28 drop-shadow-2xl md:block lg:right-5 lg:top-5 lg:size-36">
      {enabled ? <ThreeDeviceScene /> : <DeviceFallback />}
    </div>
  );
}
