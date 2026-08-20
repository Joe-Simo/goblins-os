"use client";

import { useEffect } from "react";
import gsap from "gsap";

export function MotionReveal() {
  useEffect(() => {
    const media = gsap.matchMedia();

    media.add("(prefers-reduced-motion: no-preference)", () => {
      const tweens: gsap.core.Tween[] = [];
      const heroItems = gsap.utils.toArray<HTMLElement>("[data-hero-reveal]");
      tweens.push(
        gsap.fromTo(
          heroItems,
          { opacity: 0, y: 28 },
          {
            opacity: 1,
            y: 0,
            duration: 0.72,
            ease: "power3.out",
            stagger: 0.08,
            clearProps: "opacity,transform",
          },
        ),
      );

      const revealItems = gsap.utils.toArray<HTMLElement>("[data-gsap='reveal']");
      const observer = new IntersectionObserver(
        (entries) => {
          for (const entry of entries) {
            if (!entry.isIntersecting) {
              continue;
            }

            const element = entry.target as HTMLElement;
            tweens.push(
              gsap.fromTo(
                element,
                { opacity: 0, y: 24 },
                {
                  opacity: 1,
                  y: 0,
                  duration: 0.64,
                  ease: "power3.out",
                  clearProps: "opacity,transform",
                },
              ),
            );
            observer.unobserve(element);
          }
        },
        { rootMargin: "0px 0px -10%", threshold: 0.16 },
      );

      revealItems.forEach((element) => observer.observe(element));

      return () => {
        observer.disconnect();
        tweens.forEach((tween) => tween.kill());
      };
    });

    return () => media.revert();
  }, []);

  return null;
}
