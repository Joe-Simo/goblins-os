"use client";

import { Canvas, useFrame } from "@react-three/fiber";
import { Float, PerspectiveCamera } from "@react-three/drei";
import { useRef } from "react";
import type { Mesh } from "three";

function InstallerMedia() {
  const meshRef = useRef<Mesh>(null);

  useFrame((_, delta) => {
    if (meshRef.current) {
      meshRef.current.rotation.y += delta * 0.18;
    }
  });

  return (
    <Float speed={1.2} rotationIntensity={0.25} floatIntensity={0.35}>
      <group ref={meshRef} rotation={[0.18, -0.45, 0.08]}>
        <mesh>
          <boxGeometry args={[2.2, 0.18, 1.25]} />
          <meshStandardMaterial color="#111312" roughness={0.45} metalness={0.16} />
        </mesh>
        <mesh position={[0, 0.11, 0]}>
          <boxGeometry args={[1.54, 0.035, 0.82]} />
          <meshStandardMaterial color="#0d7f63" roughness={0.36} />
        </mesh>
        <mesh position={[-0.44, 0.15, 0.01]}>
          <torusGeometry args={[0.22, 0.018, 12, 48]} />
          <meshStandardMaterial color="#f8fffc" roughness={0.28} />
        </mesh>
        <mesh position={[0.32, 0.15, 0.01]}>
          <boxGeometry args={[0.58, 0.028, 0.08]} />
          <meshStandardMaterial color="#f8fffc" roughness={0.3} />
        </mesh>
      </group>
    </Float>
  );
}

export function ThreeDeviceScene() {
  return (
    <Canvas
      dpr={[1, 1.5]}
      gl={{
        antialias: true,
        powerPreference: "low-power",
        alpha: true,
        preserveDrawingBuffer: true,
      }}
      aria-hidden="true"
    >
      <PerspectiveCamera makeDefault position={[0, 1.3, 4.4]} fov={38} />
      <ambientLight intensity={1.7} />
      <directionalLight position={[2, 3, 4]} intensity={1.4} />
      <InstallerMedia />
    </Canvas>
  );
}
