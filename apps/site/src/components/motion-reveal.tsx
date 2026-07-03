"use client";

import { useEffect } from "react";
import gsap from "gsap";

export function MotionReveal() {
  useEffect(() => {
    const prefersReducedMotion = window.matchMedia(
      "(prefers-reduced-motion: reduce)",
    ).matches;

    if (prefersReducedMotion) {
      return;
    }

    const context = gsap.context(() => {
      gsap.fromTo(
        "[data-gsap='reveal']",
        { opacity: 0, y: 18 },
        {
          opacity: 1,
          y: 0,
          clearProps: "opacity,transform",
          duration: 0.38,
          ease: "power3.out",
          stagger: { amount: 0.28 },
        },
      );
    });

    return () => context.revert();
  }, []);

  return null;
}
