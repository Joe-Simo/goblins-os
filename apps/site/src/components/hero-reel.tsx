"use client";

import { useEffect, useRef, useState } from "react";
import { PauseIcon, PlayIcon } from "lucide-react";
import { Button } from "@/components/ui/button";

export function HeroReel() {
  const videoRef = useRef<HTMLVideoElement>(null);
  const playbackRequestedRef = useRef(true);
  const inViewportRef = useRef(true);
  const [isPlaying, setIsPlaying] = useState(false);

  useEffect(() => {
    const video = videoRef.current;
    if (!video) {
      return;
    }

    const reducedMotion = window.matchMedia(
      "(prefers-reduced-motion: reduce)",
    ).matches;
    const saveData = (
      navigator as Navigator & { connection?: { saveData?: boolean } }
    ).connection?.saveData;
    const compactViewport = window.matchMedia("(max-width: 48rem)").matches;
    if (reducedMotion || saveData || compactViewport) {
      playbackRequestedRef.current = false;
      video.pause();
      setIsPlaying(false);
      return;
    }

    const syncPlayback = () => {
      if (
        playbackRequestedRef.current &&
        inViewportRef.current &&
        !document.hidden
      ) {
        void video.play().then(
          () => setIsPlaying(true),
          () => setIsPlaying(false),
        );
      } else {
        video.pause();
      }
    };

    const observer = new IntersectionObserver(
      ([entry]) => {
        inViewportRef.current = entry?.isIntersecting ?? false;
        syncPlayback();
      },
      { threshold: 0.12 },
    );
    observer.observe(video);
    document.addEventListener("visibilitychange", syncPlayback);
    syncPlayback();

    return () => {
      observer.disconnect();
      document.removeEventListener("visibilitychange", syncPlayback);
      video.pause();
    };
  }, []);

  function togglePlayback() {
    const video = videoRef.current;
    if (!video) {
      return;
    }

    if (video.paused) {
      playbackRequestedRef.current = true;
      if (inViewportRef.current && !document.hidden) {
        void video.play().then(
          () => setIsPlaying(true),
          () => setIsPlaying(false),
        );
      }
    } else {
      playbackRequestedRef.current = false;
      video.pause();
      setIsPlaying(false);
    }
  }

  return (
    <div className="hero-reel" data-window>
      <div className="window-bar" aria-hidden="true">
        <span className="window-dot window-dot--red" />
        <span className="window-dot window-dot--yellow" />
        <span className="window-dot window-dot--green" />
        <span className="window-title">Goblins OS · Product tour</span>
        <span className="window-live">Live preview</span>
      </div>
      <div className="hero-reel__screen">
        <video
          ref={videoRef}
          aria-describedby="hero-reel-caption"
          className="hero-reel__video"
          loop
          muted
          playsInline
          poster="/screenshots/home.png"
          preload="metadata"
          onPause={() => setIsPlaying(false)}
          onPlay={() => setIsPlaying(true)}
        >
          <source src="/media/goblins-os-demo.mp4" type="video/mp4" />
          The demo video is unavailable. The same experience is shown in the
          still-image chapters below.
        </video>
        <Button
          className="hero-reel__control"
          type="button"
          variant="secondary"
          size="sm"
          onClick={togglePlayback}
        >
          {isPlaying ? (
            <PauseIcon data-icon="inline-start" aria-hidden="true" />
          ) : (
            <PlayIcon data-icon="inline-start" aria-hidden="true" />
          )}
          {isPlaying ? "Pause demo" : "Play demo"}
        </Button>
      </div>
      <p id="hero-reel-caption" className="hero-reel__caption">
        A short, silent tour from the desktop into Build Studio. On smaller or
        reduced-motion screens, the tour waits for you to press play.
      </p>
    </div>
  );
}
