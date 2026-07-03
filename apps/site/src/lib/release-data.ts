export type ArchitectureId = "aarch64" | "x86_64";

export type ReleaseDownloadPart = {
  filename: string;
  url: string;
  sizeBytes: number;
  sha256: string;
};

export type ReleaseArtifact = {
  arch: ArchitectureId;
  label: string;
  cpu: string;
  isoName: string;
  compressedName: string;
  rawSizeBytes: number;
  compressedSizeBytes: number;
  sha256: string;
  compressedSha256: string;
  isoSha256Url: string;
  compressedSha256Url: string;
  partsSha256Url: string;
  manifestUrl: string;
  evidenceUrl: string;
  downloadParts: ReleaseDownloadPart[];
  builtOn: string;
  status: "available" | "blocked";
  notes: string[];
};

export type ContainerImage = {
  arch: ArchitectureId;
  label: string;
  image: string;
  platform: string;
  sourceManifestUrl: string;
  pullCommand: string;
  verifyCommand: string;
  status: "public" | "visibility_pending";
  note: string;
};

const releaseTag = "v0.1.0-alpha.20260703";
const releaseBaseUrl =
  "https://github.com/Joe-Simo/goblins-os/releases/download/v0.1.0-alpha.20260703";

const releaseAssetUrl = (filename: string) => `${releaseBaseUrl}/${filename}`;

export const releaseArtifacts = [
  {
    arch: "aarch64",
    label: "Arm / aarch64",
    cpu: "Native Arm systems and Arm virtual machines.",
    isoName: "goblins-os-aarch64.iso",
    compressedName: "goblins-os-aarch64.iso.zst",
    rawSizeBytes: 2861367296,
    compressedSizeBytes: 2553550869,
    sha256: "13b2b59ea03054d66b3f8c0986c2314631437e57074685c515a1dffa3a4f6fbf",
    compressedSha256:
      "652a85446c675c958d6175a4468a2ae1af716fbd182d4d320be576aec4dfac31",
    isoSha256Url: releaseAssetUrl("goblins-os-aarch64.iso.sha256"),
    compressedSha256Url: releaseAssetUrl("goblins-os-aarch64.iso.zst.sha256"),
    partsSha256Url: releaseAssetUrl("goblins-os-aarch64.iso.zst.parts.sha256"),
    manifestUrl: releaseAssetUrl("manifest-goblins-os-aarch64.json"),
    evidenceUrl: releaseAssetUrl("release-evidence-manifest-aarch64.json"),
    downloadParts: [
      {
        filename: "goblins-os-aarch64.iso.zst.part-00",
        url: releaseAssetUrl("goblins-os-aarch64.iso.zst.part-00"),
        sizeBytes: 1887436800,
        sha256:
          "5deade48e9fc1eabe99be1a180c6f690eabf6c31a1bc9320479970f4c1727618",
      },
      {
        filename: "goblins-os-aarch64.iso.zst.part-01",
        url: releaseAssetUrl("goblins-os-aarch64.iso.zst.part-01"),
        sizeBytes: 666114069,
        sha256:
          "450773c794e9aa4b6a8dd81a3f5831b93282656f0ab799e84004dfbff1d8c461",
      },
    ],
    builtOn: "2026-07-03T18:19:13Z",
    status: "available",
    notes: [
      "Alpha release. Use a spare device or VM and back up first.",
      "Full release signoff is still in progress.",
    ],
  },
  {
    arch: "x86_64",
    label: "Intel / AMD x86_64",
    cpu: "64-bit Intel and AMD systems.",
    isoName: "goblins-os-x86_64.iso",
    compressedName: "goblins-os-x86_64.iso.zst",
    rawSizeBytes: 3164340224,
    compressedSizeBytes: 2767398792,
    sha256: "45abf064735fa2a2ba9ef034883d19453c4bfc02a3b0c311d29e3679c52db434",
    compressedSha256:
      "c433bb73fc4da1629f86eed9b908f8f2dc9c200e56dbc54b8f2185d90f809d68",
    isoSha256Url: releaseAssetUrl("goblins-os-x86_64.iso.sha256"),
    compressedSha256Url: releaseAssetUrl("goblins-os-x86_64.iso.zst.sha256"),
    partsSha256Url: releaseAssetUrl("goblins-os-x86_64.iso.zst.parts.sha256"),
    manifestUrl: releaseAssetUrl("manifest-goblins-os-x86_64.json"),
    evidenceUrl: releaseAssetUrl("release-evidence-manifest-x86_64.json"),
    downloadParts: [
      {
        filename: "goblins-os-x86_64.iso.zst.part-00",
        url: releaseAssetUrl("goblins-os-x86_64.iso.zst.part-00"),
        sizeBytes: 1887436800,
        sha256:
          "40b7fcf8216b3a3b08e3f4d0cc791b413c3c85e1cd8d81c152a0455e25f536dc",
      },
      {
        filename: "goblins-os-x86_64.iso.zst.part-01",
        url: releaseAssetUrl("goblins-os-x86_64.iso.zst.part-01"),
        sizeBytes: 879961992,
        sha256:
          "7dd74eb52891389579d83f7a23ab30e06d00a2f7643a621b56c247f0911abc81",
      },
    ],
    builtOn: "2026-07-03T18:21:10Z",
    status: "available",
    notes: [
      "Alpha release. Use a spare device or VM and back up first.",
      "Full release signoff is still in progress.",
    ],
  },
] satisfies ReleaseArtifact[];

export const containerImages = [
  {
    arch: "aarch64",
    label: "Arm / aarch64 bootc image",
    image: "ghcr.io/joe-simo/goblins-os:aarch64",
    platform: "linux/arm64",
    sourceManifestUrl: releaseAssetUrl("manifest-goblins-os-aarch64.json"),
    pullCommand: "docker pull ghcr.io/joe-simo/goblins-os:aarch64",
    verifyCommand:
      "docker run --rm ghcr.io/joe-simo/goblins-os:aarch64 /usr/libexec/goblins-os/goblins-os-verify",
    status: "visibility_pending",
    note: "The image has been published; public pulls are waiting on GitHub Container Registry package visibility.",
  },
  {
    arch: "x86_64",
    label: "Intel / AMD x86_64 bootc image",
    image: "ghcr.io/joe-simo/goblins-os:x86_64",
    platform: "linux/amd64",
    sourceManifestUrl: releaseAssetUrl("manifest-goblins-os-x86_64.json"),
    pullCommand: "docker pull ghcr.io/joe-simo/goblins-os:x86_64",
    verifyCommand:
      "docker run --rm ghcr.io/joe-simo/goblins-os:x86_64 /usr/libexec/goblins-os/goblins-os-verify",
    status: "visibility_pending",
    note: "The image has been published; public pulls are waiting on GitHub Container Registry package visibility.",
  },
] satisfies ContainerImage[];

export const releaseEvidence = {
  source: "GitHub release assets",
  sourcePolicy: "SHIP.md",
  architecturePolicy: "os/release/architectures.toml",
  releaseWorkflow: ".github/workflows/release.yml",
  releaseTag,
  releaseUrl: "https://github.com/Joe-Simo/goblins-os/releases/tag/v0.1.0-alpha.20260703",
  releaseRunUrl: "https://github.com/Joe-Simo/goblins-os/actions/runs/28676213034",
  targetCommit: "e89b1912d1b54bd6710c410c615827059cf8527a",
  publishedAt: "2026-07-03T18:52:36Z",
  bandwidthPolicy:
    "Large ISO downloads resolve to GitHub release assets, not Vercel static assets.",
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
