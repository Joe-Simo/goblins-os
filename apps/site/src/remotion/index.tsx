import { registerRoot } from "remotion";
import { Composition, Img, interpolate, staticFile, useCurrentFrame } from "remotion";

const frames = [
  "screenshots/home.png",
  "screenshots/build-studio.png",
  "screenshots/workspace-overview.png",
  "screenshots/installer-disk.png",
];

function GoblinsOsDemo() {
  const frame = useCurrentFrame();
  const index = Math.min(frames.length - 1, Math.floor(frame / 45));
  const progress = frame % 45;
  const opacity = interpolate(progress, [0, 8, 37, 45], [0, 1, 1, 0], {
    extrapolateLeft: "clamp",
    extrapolateRight: "clamp",
  });

  return (
    <div
      style={{
        width: "100%",
        height: "100%",
        background: "#f7f8f8",
        display: "flex",
        alignItems: "center",
        justifyContent: "center",
        fontFamily: "Inter, sans-serif",
      }}
    >
      <div
        style={{
          width: 1500,
          height: 844,
          border: "1px solid #d9dada",
          borderRadius: 20,
          overflow: "hidden",
          boxShadow: "0 34px 90px rgba(0,0,0,0.18)",
          opacity,
        }}
      >
        <Img src={staticFile(frames[index])} style={{ width: "100%", height: "100%", objectFit: "cover" }} />
      </div>
    </div>
  );
}

function RemotionRoot() {
  return (
    <Composition
      id="GoblinsOsDemo"
      component={GoblinsOsDemo}
      durationInFrames={180}
      fps={30}
      width={1920}
      height={1080}
    />
  );
}

registerRoot(RemotionRoot);
