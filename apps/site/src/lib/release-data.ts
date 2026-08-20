export type ArchitectureId = "aarch64" | "x86_64";
export type CurrentReleaseArchitectureId = "aarch64";
export type HistoricalReleaseArchitectureId = ArchitectureId;

export type ReleaseDownloadPart = {
  filename: string;
  url: string;
  sizeBytes: number;
  sha256: string;
};

export type ReleaseArtifact<TArchitecture extends ArchitectureId = ArchitectureId> = {
  arch: TArchitecture;
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
  status: "preview";
  notes: string[];
};

export type HistoricalReleaseArtifact = Omit<
  ReleaseArtifact<HistoricalReleaseArchitectureId>,
  "status"
> & {
  status: "historical";
  releaseTag: string;
  releaseUrl: string;
  recordNote: string;
};

export type ContainerImage = {
  arch: CurrentReleaseArchitectureId;
  label: string;
  image: string;
  platform: string;
  sourceManifestUrl: string;
  pullCommand: string;
  verifyCommand: string;
  podmanPullCommand: string;
  podmanVerifyCommand: string;
  status: "public";
  note: string;
};

const releaseTag = "v0.2.0-preview.20260820";
const releaseBaseUrl =
  "https://github.com/Joe-Simo/goblins-os/releases/download/v0.2.0-preview.20260820";
const historicalX86ReleaseTag = "v0.1.0-alpha.20260703";
const historicalX86ReleaseUrl =
  "https://github.com/Joe-Simo/goblins-os/releases/tag/v0.1.0-alpha.20260703";
const historicalX86ReleaseBaseUrl =
  "https://github.com/Joe-Simo/goblins-os/releases/download/v0.1.0-alpha.20260703";

const releaseAssetUrl = (filename: string) => `${releaseBaseUrl}/${filename}`;
const historicalReleaseAssetUrl = (filename: string) =>
  `${historicalX86ReleaseBaseUrl}/${filename}`;

export const releaseArtifacts = [
  {
    arch: "aarch64",
    label: "Goblins OS 0.2 preview · Arm / aarch64",
    cpu: "UEFI aarch64 virtual machines. Bare-metal devices remain experimental.",
    isoName: "goblins-os-aarch64.iso",
    compressedName: "goblins-os-aarch64.iso.zst",
    rawSizeBytes: 3026386944,
    compressedSizeBytes: 2691604357,
    sha256: "4a42876fd693bad1602f7ea60a4e2b48975f10417abec542f97b1cfab279b402",
    compressedSha256:
      "2c7f00763e7f5f2556a6e16ff036c6ed44324467b1c84849db227dfd8f566c58",
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
          "23d6fd9fbd8b36e8f3bb4d4f2b123cd8c61c62fd4e758814e2ff6ed6b883b5c9",
      },
      {
        filename: "goblins-os-aarch64.iso.zst.part-01",
        url: releaseAssetUrl("goblins-os-aarch64.iso.zst.part-01"),
        sizeBytes: 804167557,
        sha256:
          "908aa43abd20c4cde50e7bdc6c5a1160bb11f81a659e7e78b580dca2bbfef622",
      },
    ],
    builtOn: "2026-08-20T17:58:20Z",
    status: "preview",
    notes: [
      "Installable community preview for exploration, demos, and feedback.",
      "Update-enabled and checksum-bound, but not the formally signed stable release.",
    ],
  },
] satisfies ReleaseArtifact<CurrentReleaseArchitectureId>[];

export const historicalReleaseArtifacts = [
  {
    arch: "aarch64",
    label: "Historical Arm / aarch64 alpha",
    cpu: "Immutable provenance for the retired July 2026 Arm alpha.",
    isoName: "goblins-os-aarch64.iso",
    compressedName: "goblins-os-aarch64.iso.zst",
    rawSizeBytes: 2861367296,
    compressedSizeBytes: 2553550869,
    sha256: "13b2b59ea03054d66b3f8c0986c2314631437e57074685c515a1dffa3a4f6fbf",
    compressedSha256:
      "652a85446c675c958d6175a4468a2ae1af716fbd182d4d320be576aec4dfac31",
    isoSha256Url: historicalReleaseAssetUrl("goblins-os-aarch64.iso.sha256"),
    compressedSha256Url: historicalReleaseAssetUrl(
      "goblins-os-aarch64.iso.zst.sha256",
    ),
    partsSha256Url: historicalReleaseAssetUrl(
      "goblins-os-aarch64.iso.zst.parts.sha256",
    ),
    manifestUrl: historicalReleaseAssetUrl("manifest-goblins-os-aarch64.json"),
    evidenceUrl: historicalReleaseAssetUrl(
      "release-evidence-manifest-aarch64.json",
    ),
    downloadParts: [
      {
        filename: "goblins-os-aarch64.iso.zst.part-00",
        url: historicalReleaseAssetUrl("goblins-os-aarch64.iso.zst.part-00"),
        sizeBytes: 1887436800,
        sha256:
          "5deade48e9fc1eabe99be1a180c6f690eabf6c31a1bc9320479970f4c1727618",
      },
      {
        filename: "goblins-os-aarch64.iso.zst.part-01",
        url: historicalReleaseAssetUrl("goblins-os-aarch64.iso.zst.part-01"),
        sizeBytes: 666114069,
        sha256:
          "450773c794e9aa4b6a8dd81a3f5831b93282656f0ab799e84004dfbff1d8c461",
      },
    ],
    builtOn: "2026-07-03T18:19:13Z",
    status: "historical",
    releaseTag: historicalX86ReleaseTag,
    releaseUrl: historicalX86ReleaseUrl,
    recordNote:
      "Retained only as an immutable historical record. It is not supported or current.",
    notes: [
      "Originally published as part of the July 2026 alpha.",
      "Checksums, manifests, and evidence remain available for provenance.",
    ],
  },
  {
    arch: "x86_64",
    label: "Historical Intel / AMD x86_64 alpha",
    cpu: "Immutable provenance for the retired Intel and AMD alpha target.",
    isoName: "goblins-os-x86_64.iso",
    compressedName: "goblins-os-x86_64.iso.zst",
    rawSizeBytes: 3164340224,
    compressedSizeBytes: 2767398792,
    sha256: "45abf064735fa2a2ba9ef034883d19453c4bfc02a3b0c311d29e3679c52db434",
    compressedSha256:
      "c433bb73fc4da1629f86eed9b908f8f2dc9c200e56dbc54b8f2185d90f809d68",
    isoSha256Url: historicalReleaseAssetUrl("goblins-os-x86_64.iso.sha256"),
    compressedSha256Url: historicalReleaseAssetUrl(
      "goblins-os-x86_64.iso.zst.sha256",
    ),
    partsSha256Url: historicalReleaseAssetUrl(
      "goblins-os-x86_64.iso.zst.parts.sha256",
    ),
    manifestUrl: historicalReleaseAssetUrl("manifest-goblins-os-x86_64.json"),
    evidenceUrl: historicalReleaseAssetUrl(
      "release-evidence-manifest-x86_64.json",
    ),
    downloadParts: [
      {
        filename: "goblins-os-x86_64.iso.zst.part-00",
        url: historicalReleaseAssetUrl("goblins-os-x86_64.iso.zst.part-00"),
        sizeBytes: 1887436800,
        sha256:
          "40b7fcf8216b3a3b08e3f4d0cc791b413c3c85e1cd8d81c152a0455e25f536dc",
      },
      {
        filename: "goblins-os-x86_64.iso.zst.part-01",
        url: historicalReleaseAssetUrl("goblins-os-x86_64.iso.zst.part-01"),
        sizeBytes: 879961992,
        sha256:
          "7dd74eb52891389579d83f7a23ab30e06d00a2f7643a621b56c247f0911abc81",
      },
    ],
    builtOn: "2026-07-03T18:21:10Z",
    status: "historical",
    releaseTag: historicalX86ReleaseTag,
    releaseUrl: historicalX86ReleaseUrl,
    recordNote:
      "Retained only as an immutable historical record. It is not supported, current, or eligible for promotion.",
    notes: [
      "Originally published as part of the July 2026 alpha.",
      "Checksums, manifests, and evidence remain available for provenance.",
    ],
  },
] satisfies HistoricalReleaseArtifact[];

export const containerImages = [
  {
    arch: "aarch64",
    label: "Goblins OS image · Arm / aarch64",
    image: "ghcr.io/joe-simo/goblins-os:aarch64",
    platform: "linux/arm64",
    sourceManifestUrl: releaseAssetUrl("manifest-goblins-os-aarch64.json"),
    pullCommand: "docker pull ghcr.io/joe-simo/goblins-os:aarch64",
    verifyCommand:
      "docker run --rm ghcr.io/joe-simo/goblins-os:aarch64 /usr/libexec/goblins-os/goblins-os-verify",
    podmanPullCommand: "podman pull ghcr.io/joe-simo/goblins-os:aarch64",
    podmanVerifyCommand:
      "podman run --rm ghcr.io/joe-simo/goblins-os:aarch64 /usr/libexec/goblins-os/goblins-os-verify",
    status: "public",
    note: "Public update channel for the 0.2 preview image. Existing aarch64 installs track this channel through bootc.",
  },
] satisfies ContainerImage[];

export const releaseEvidence = {
  source: "GitHub release assets",
  sourcePolicy: "SHIP.md",
  architecturePolicy: "os/release/architectures.toml",
  releaseWorkflow: ".github/workflows/aarch64-preview-iso.yml",
  releaseTag,
  releaseUrl:
    "https://github.com/Joe-Simo/goblins-os/releases/tag/v0.2.0-preview.20260820",
  releaseRunUrl: "https://github.com/Joe-Simo/goblins-os/actions/runs/32399132599",
  targetCommit: "8640ca9ff65f4cef66bf2f56e442fdebdb48093e",
  publishedAt: "2026-08-20T18:09:21Z",
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
