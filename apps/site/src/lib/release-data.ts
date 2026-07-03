export type ArchitectureId = "aarch64" | "x86_64";

export type ReleaseArtifact = {
  arch: ArchitectureId;
  label: string;
  cpu: string;
  isoName: string;
  expectedIsoPath: string;
  expectedSha256Path: string;
  expectedManifestPath: string;
  proofManifestPath: string | null;
  publicReleaseUrl: string | null;
  sizeBytes: number | null;
  sha256: string | null;
  lastUpdated: string | null;
  status: "available" | "blocked";
  blockers: string[];
};

export const releaseArtifacts = [
  {
    arch: "aarch64",
    label: "Arm / aarch64",
    cpu: "Native Arm systems and Arm virtual machines.",
    isoName: "goblins-os-aarch64.iso",
    expectedIsoPath: "os/iso/output/aarch64/bootiso/goblins-os-aarch64.iso",
    expectedSha256Path:
      "os/iso/output/aarch64/bootiso/goblins-os-aarch64.iso.sha256",
    expectedManifestPath: "os/iso/output/aarch64/manifest-goblins-os-aarch64.json",
    proofManifestPath: null,
    publicReleaseUrl: null,
    sizeBytes: null,
    sha256: null,
    lastUpdated: null,
    status: "blocked",
    blockers: [
      "Missing checked artifact: os/iso/output/aarch64/bootiso/goblins-os-aarch64.iso",
      "Missing public release URL for a low-cost artifact host.",
      "Missing current aarch64 proof manifest in os/screenshots/hardware-gate/aarch64/<date>/.",
    ],
  },
  {
    arch: "x86_64",
    label: "Intel / AMD x86_64",
    cpu: "64-bit Intel and AMD systems.",
    isoName: "goblins-os-x86_64.iso",
    expectedIsoPath: "os/iso/output/x86_64/bootiso/goblins-os-x86_64.iso",
    expectedSha256Path:
      "os/iso/output/x86_64/bootiso/goblins-os-x86_64.iso.sha256",
    expectedManifestPath: "os/iso/output/x86_64/manifest-goblins-os-x86_64.json",
    proofManifestPath:
      "os/screenshots/hardware-gate/x86_64/2026-07-03/proof-manifest.json",
    publicReleaseUrl: null,
    sizeBytes: null,
    sha256:
      "85d34b5c864ee643768e5ca6db7bc149f67319f3be76acda6f4901714a0f99fb",
    lastUpdated: "2026-07-03",
    status: "blocked",
    blockers: [
      "Missing checked artifact: os/iso/output/x86_64/bootiso/goblins-os-x86_64.iso",
      "Missing public release URL for a low-cost artifact host.",
      "Checksum exists only in proof metadata until the ISO and .sha256 file are present.",
    ],
  },
] satisfies ReleaseArtifact[];

export const releaseEvidence = {
  source: "Current worktree evidence",
  sourcePolicy: "SHIP.md",
  architecturePolicy: "os/release/architectures.toml",
  releaseWorkflow: ".github/workflows/release.yml",
  bandwidthPolicy:
    "Large ISO downloads must resolve to a public release host, not Vercel static assets.",
  releaseNotesUrl: null,
};

export function formatBytes(value: number | null) {
  if (value === null) {
    return "Not available";
  }

  const units = ["B", "KB", "MB", "GB", "TB"] as const;
  let size = value;
  let unitIndex = 0;

  while (size >= 1024 && unitIndex < units.length - 1) {
    size /= 1024;
    unitIndex += 1;
  }

  return `${size.toFixed(size >= 10 || unitIndex === 0 ? 0 : 1)} ${units[unitIndex]}`;
}
