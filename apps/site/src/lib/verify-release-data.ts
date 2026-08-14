import { readFile, stat } from "node:fs/promises";
import path from "node:path";
import {
  containerImages,
  historicalReleaseArtifacts,
  releaseArtifacts,
  releaseEvidence,
} from "./release-data";
import { assetBudget, screenshots } from "./site-assets";

function assert(condition: unknown, message: string): asserts condition {
  if (!condition) {
    throw new Error(message);
  }
}

function equalStringSets(actual: string[], expected: string[], message: string) {
  assert(
    JSON.stringify([...actual].sort()) === JSON.stringify([...expected].sort()),
    `${message}: expected ${expected.join(", ")}; received ${actual.join(", ")}`,
  );
}

const sha256Pattern = /^[0-9a-f]{64}$/;
const commitPattern = /^[0-9a-f]{40}$/;

function assertSha256(value: string, label: string) {
  assert(
    sha256Pattern.test(value),
    `${label} is not a canonical lowercase SHA256`,
  );
}

equalStringSets(
  releaseArtifacts.map(({ arch }) => arch),
  ["aarch64"],
  "Current release architecture drifted",
);
equalStringSets(
  containerImages.map(({ arch }) => arch),
  ["aarch64"],
  "Current container architecture drifted",
);
equalStringSets(
  historicalReleaseArtifacts.map(({ arch }) => arch),
  ["x86_64"],
  "Historical release architecture drifted",
);

for (const artifact of releaseArtifacts) {
  const releaseBaseUrl = `https://github.com/Joe-Simo/goblins-os/releases/download/${releaseEvidence.releaseTag}`;
  assert(
    artifact.status === "historical",
    `Preserved ${artifact.arch} media must remain explicitly historical`,
  );
  assert(
    Number.isSafeInteger(artifact.rawSizeBytes) && artifact.rawSizeBytes > 0,
    "Raw ISO size is invalid",
  );
  assert(
    Number.isSafeInteger(artifact.compressedSizeBytes) && artifact.compressedSizeBytes > 0,
    "Compressed ISO size is invalid",
  );
  assertSha256(artifact.sha256, `${artifact.arch} raw ISO digest`);
  assertSha256(artifact.compressedSha256, `${artifact.arch} compressed ISO digest`);
  assert(
    artifact.isoName === `goblins-os-${artifact.arch}.iso`,
    `Current ${artifact.arch} ISO name is not architecture-bound`,
  );
  assert(
    artifact.compressedName === `${artifact.isoName}.zst`,
    `Current ${artifact.arch} compressed filename is inconsistent`,
  );
  assert(
    artifact.downloadParts.length > 0 &&
      artifact.downloadParts.every(({ filename }) => filename.includes(`-${artifact.arch}.`)),
    `Current ${artifact.arch} split media is not architecture-bound`,
  );
  assert(
    artifact.downloadParts.reduce((total, part) => total + part.sizeBytes, 0) ===
      artifact.compressedSizeBytes,
    `Current ${artifact.arch} split media size does not reconstruct the compressed ISO`,
  );
  artifact.downloadParts.forEach((part, index) => {
    const expectedName = `${artifact.compressedName}.part-${index.toString().padStart(2, "0")}`;
    assert(
      part.filename === expectedName,
      `${artifact.arch} part order or filename drifted`,
    );
    assert(
      part.url === `${releaseBaseUrl}/${expectedName}`,
      `${expectedName} URL drifted`,
    );
    assert(
      Number.isSafeInteger(part.sizeBytes) && part.sizeBytes > 0,
      `${expectedName} size is invalid`,
    );
    assertSha256(part.sha256, `${expectedName} digest`);
  });
  assert(
    artifact.isoSha256Url === `${releaseBaseUrl}/${artifact.isoName}.sha256`,
    `Current ${artifact.arch} raw checksum URL is inconsistent`,
  );
  assert(
    artifact.compressedSha256Url === `${releaseBaseUrl}/${artifact.compressedName}.sha256`,
    `Current ${artifact.arch} compressed checksum URL is inconsistent`,
  );
  assert(
    artifact.partsSha256Url === `${releaseBaseUrl}/${artifact.compressedName}.parts.sha256`,
    `Current ${artifact.arch} parts checksum URL is inconsistent`,
  );
  assert(
    artifact.manifestUrl === `${releaseBaseUrl}/manifest-goblins-os-${artifact.arch}.json`,
    `Current ${artifact.arch} manifest URL is inconsistent`,
  );
  assert(
    artifact.evidenceUrl ===
      `${releaseBaseUrl}/release-evidence-manifest-${artifact.arch}.json`,
    `Current ${artifact.arch} evidence URL is inconsistent`,
  );
}

for (const artifact of historicalReleaseArtifacts) {
  assert(artifact.status === "historical", `${artifact.arch} must remain historical`);
  assertSha256(artifact.sha256, `${artifact.arch} historical raw ISO digest`);
  assertSha256(
    artifact.compressedSha256,
    `${artifact.arch} historical compressed ISO digest`,
  );
  assert(
    artifact.downloadParts.reduce((total, part) => total + part.sizeBytes, 0) ===
      artifact.compressedSizeBytes,
    `${artifact.arch} historical split media size is inconsistent`,
  );
  artifact.downloadParts.forEach((part) =>
    assertSha256(part.sha256, `${part.filename} digest`),
  );
  assert(
    artifact.recordNote.toLowerCase().includes("not supported"),
    `${artifact.arch} historical record does not state its support boundary`,
  );
}

for (const image of containerImages) {
  assert(image.platform === "linux/arm64", `${image.arch} container platform drifted`);
  assert(image.image.endsWith(":aarch64"), `${image.arch} container tag drifted`);
  for (const command of [
    image.pullCommand,
    image.verifyCommand,
    image.podmanPullCommand,
    image.podmanVerifyCommand,
  ]) {
    assert(command.includes(":aarch64"), `${image.arch} command is not architecture-bound`);
    assert(!command.includes("x86_64") && !command.includes("amd64"), "Current command exposes x86");
  }
}

assert(
  containerImages.length === releaseArtifacts.length,
  "Container and release counts differ",
);
for (const artifact of releaseArtifacts) {
  const image = containerImages.find(({ arch }) => arch === artifact.arch);
  assert(image, `${artifact.arch} release has no matching container`);
  assert(
    image.sourceManifestUrl === artifact.manifestUrl,
    `${artifact.arch} container and release manifest URLs differ`,
  );
}

assert(
  releaseEvidence.architecturePolicy === "os/release/architectures.toml",
  "Website release evidence lost the repository architecture policy",
);
assert(
  commitPattern.test(releaseEvidence.targetCommit),
  "Release target commit is not canonical",
);
assert(
  releaseEvidence.releaseUrl ===
    `https://github.com/Joe-Simo/goblins-os/releases/tag/${releaseEvidence.releaseTag}`,
  "Release tag and release URL differ",
);
assert(
  /^https:\/\/github\.com\/Joe-Simo\/goblins-os\/actions\/runs\/[1-9][0-9]*$/.test(
    releaseEvidence.releaseRunUrl,
  ),
  "Release run URL is not canonical",
);

const pngSignature = Buffer.from([137, 80, 78, 71, 13, 10, 26, 10]);
const publicDir = path.join(process.cwd(), "public");
let screenshotBytes = 0;

for (const screenshot of screenshots) {
  const filePath = path.join(publicDir, screenshot.src.replace(/^\//, ""));
  const [metadata, bytes] = await Promise.all([stat(filePath), readFile(filePath)]);
  assert(metadata.isFile(), `${screenshot.src} is not a regular file`);
  assert(metadata.size === screenshot.bytes, `${screenshot.src} byte count drifted`);
  assert(bytes.subarray(0, 8).equals(pngSignature), `${screenshot.src} is not a PNG`);
  assert(bytes.length >= 24, `${screenshot.src} is too short to contain a PNG header`);
  assert(bytes.toString("ascii", 12, 16) === "IHDR", `${screenshot.src} lacks PNG IHDR`);
  assert(bytes.readUInt32BE(16) === screenshot.width, `${screenshot.src} width drifted`);
  assert(bytes.readUInt32BE(20) === screenshot.height, `${screenshot.src} height drifted`);
  screenshotBytes += metadata.size;
}

assert(
  screenshotBytes === assetBudget.screenshotBytes,
  "Screenshot asset budget differs from the verified files",
);

console.log(
  `release-data: pass public-channel=aarch64 historical-media=aarch64,x86_64 screenshots=${screenshots.length} bytes=${screenshotBytes}`,
);
