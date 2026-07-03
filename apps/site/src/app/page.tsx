import Image from "next/image";
import {
  ArrowDownToLineIcon,
  ArrowRightIcon,
  ArrowUpRightIcon,
  BoxIcon,
  CheckCircle2Icon,
  CodeIcon,
  CpuIcon,
  ExternalLinkIcon,
  FileCheck2Icon,
  Gamepad2Icon,
  HardDriveIcon,
  LockKeyholeIcon,
  MonitorIcon,
  ShieldCheckIcon,
  TerminalSquareIcon,
} from "lucide-react";
import { Alert, AlertDescription, AlertTitle } from "@/components/ui/alert";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import {
  Card,
  CardContent,
  CardDescription,
  CardFooter,
  CardHeader,
  CardTitle,
} from "@/components/ui/card";
import { Separator } from "@/components/ui/separator";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table";
import { CopyButton } from "@/components/copy-button";
import { DevicePreview } from "@/components/device-preview";
import { MotionReveal } from "@/components/motion-reveal";
import { assetBudget, screenshots } from "@/lib/site-assets";
import {
  containerImages,
  formatBytes,
  releaseArtifacts,
  releaseEvidence,
} from "@/lib/release-data";
import type { ContainerImage } from "@/lib/release-data";

const sourceUrl = "https://github.com/Joe-Simo/goblins-os";

const features = [
  {
    title: "Immutable Fedora bootc base",
    description: "Image-based updates, rollback, and native Linux packaging.",
    icon: BoxIcon,
  },
  {
    title: "Build apps locally",
    description: "Describe an app, review the generated project, and keep the output on the machine.",
    icon: TerminalSquareIcon,
  },
  {
    title: "Server-side secrets only",
    description: "The image ships without credentials; provider keys stay outside the desktop session.",
    icon: LockKeyholeIcon,
  },
  {
    title: "Native desktop surfaces",
    description: "Rust, GNOME technologies, systemd services, and real installed pixels.",
    icon: MonitorIcon,
  },
  {
    title: "Architecture-specific media",
    description: "Arm and x86_64 use separate native release media.",
    icon: CpuIcon,
  },
  {
    title: "Gaming substrate without Steam",
    description: "Mesa, Vulkan tooling, GameMode, gamescope, MangoHud, and PipeWire diagnostics.",
    icon: Gamepad2Icon,
  },
];

const installSteps = [
  {
    title: "Choose the right ISO",
    body: "Use the installer media that matches the target CPU: Arm/aarch64 or Intel/AMD x86_64.",
  },
  {
    title: "Verify before flashing",
    body: "Check the SHA256 file after download. Do not flash media when the checksum does not match.",
  },
  {
    title: "Back up and boot",
    body: "Use a USB flashing tool, boot from the installer, and choose storage deliberately.",
  },
];

export default function Home() {
  const totalMedia = formatBytes(assetBudget.screenshotBytes);
  const demoMedia = formatBytes(assetBudget.demoVideoBytes);

  return (
    <main className="min-h-screen bg-background text-foreground">
      <MotionReveal />
      <SiteHeader />

      <section className="relative overflow-hidden border-b bg-background">
        <div className="mx-auto grid w-full max-w-7xl grid-cols-1 items-center gap-8 px-4 py-8 sm:px-6 sm:py-10 lg:min-h-[680px] lg:grid-cols-[0.88fr_1.12fr] lg:gap-10 lg:px-8 lg:py-16">
          <div className="flex max-w-2xl flex-col gap-6" data-gsap="reveal">
            <div className="flex flex-col gap-4">
              <Badge variant="secondary" className="w-fit">
                Fedora bootc · native desktop · local builds
              </Badge>
              <h1 className="text-5xl font-semibold leading-[0.95] tracking-normal text-balance sm:text-6xl lg:text-7xl">
                Goblins OS
              </h1>
              <p className="max-w-xl text-base leading-7 text-muted-foreground sm:text-lg">
                A Fedora bootc desktop for building local apps. Choose the
                right architecture, verify the release media, and keep your
                system under your control.
              </p>
            </div>
            <div className="flex flex-col gap-3 sm:flex-row">
              <Button asChild size="lg">
                <a href="#downloads">
                  Check downloads
                  <ArrowDownToLineIcon data-icon="inline-end" />
                </a>
              </Button>
              <Button asChild variant="outline" size="lg">
                <a href={sourceUrl} rel="noreferrer" target="_blank">
                  View source
                  <CodeIcon data-icon="inline-end" />
                </a>
              </Button>
            </div>
            <div className="grid gap-3 text-sm text-muted-foreground sm:grid-cols-3">
              <ProofPoint>Fedora bootc base</ProofPoint>
              <ProofPoint>No bundled secrets</ProofPoint>
              <ProofPoint>Per-arch ISOs</ProofPoint>
            </div>
          </div>

          <div className="relative" data-gsap="reveal">
            <DevicePreview />
            <div className="relative rounded-lg border bg-card p-2 shadow-2xl shadow-foreground/10">
              <div className="overflow-hidden rounded-md border bg-muted">
                <Image
                  src={screenshots[0].src}
                  alt={screenshots[0].alt}
                  width={screenshots[0].width}
                  height={screenshots[0].height}
                  priority
                  sizes="(min-width: 1024px) 58vw, 100vw"
                  className="h-auto w-full"
                />
              </div>
            </div>
          </div>
        </div>
      </section>

      <section id="features" className="scroll-mt-20 border-b bg-muted/35">
        <div className="mx-auto flex w-full max-w-7xl flex-col gap-8 px-4 py-12 sm:px-6 lg:px-8">
          <SectionHeading
            title="Built for creativity and control"
            description="A native Linux desktop with verified release artifacts, per-architecture media, and a clean credential boundary."
          />
          <div className="grid gap-x-8 gap-y-0 md:grid-cols-2 lg:grid-cols-3">
            {features.map((feature) => (
              <div
                key={feature.title}
                className="flex gap-4 border-t py-6"
                data-gsap="reveal"
              >
                <feature.icon className="mt-0.5 text-primary" aria-hidden="true" />
                <div className="flex flex-col gap-2">
                  <h3 className="text-base font-semibold">{feature.title}</h3>
                  <p className="text-sm leading-6 text-muted-foreground">
                    {feature.description}
                  </p>
                </div>
              </div>
            ))}
          </div>
        </div>
      </section>

      <section id="screenshots" className="scroll-mt-20 border-b bg-background">
        <div className="mx-auto flex w-full max-w-7xl flex-col gap-8 px-4 py-12 sm:px-6 lg:px-8">
          <div className="flex flex-col justify-between gap-4 md:flex-row md:items-end">
            <SectionHeading
              title="See Goblins OS in action"
              description={`Screenshot media totals ${totalMedia}; media below the hero is lazy-loaded.`}
            />
            <Button asChild variant="ghost">
              <a href="#install">
                Install notes
                <ArrowRightIcon data-icon="inline-end" />
              </a>
            </Button>
          </div>
          <div className="grid gap-4 md:grid-cols-2 lg:grid-cols-4">
            {screenshots.map((screenshot, index) => (
              <Card key={screenshot.src} className="overflow-hidden py-0" data-gsap="reveal">
                <div className="aspect-[16/9] overflow-hidden bg-muted">
                  <Image
                    src={screenshot.src}
                    alt={screenshot.alt}
                    width={screenshot.width}
                    height={screenshot.height}
                    loading={index === 0 ? "eager" : "lazy"}
                    sizes="(min-width: 1024px) 25vw, (min-width: 768px) 50vw, 100vw"
                    className="h-full w-full object-cover"
                  />
                </div>
                <CardHeader className="px-4 py-4">
                  <CardTitle>{screenshot.title}</CardTitle>
                  <CardDescription>{screenshot.description}</CardDescription>
                </CardHeader>
              </Card>
            ))}
          </div>
          <Card data-gsap="reveal">
            <CardHeader>
              <CardTitle>Demo reel</CardTitle>
              <CardDescription>
                Built from the screenshots above. The MP4 is {demoMedia}, uses controls, and does not autoplay.
              </CardDescription>
            </CardHeader>
            <CardContent>
              <video
                className="aspect-video w-full rounded-lg border bg-muted"
                controls
                preload="metadata"
                poster="/screenshots/home.png"
              >
                <source src="/media/goblins-os-demo.mp4" type="video/mp4" />
              </video>
            </CardContent>
          </Card>
        </div>
      </section>

      <section id="downloads" className="scroll-mt-20 border-b bg-muted/35">
        <div className="mx-auto flex w-full max-w-7xl flex-col gap-8 px-4 py-12 sm:px-6 lg:px-8">
          <div className="flex flex-col justify-between gap-4 lg:flex-row lg:items-end">
            <SectionHeading
              title="Downloads"
              description="Installer media is architecture-specific. Large files are hosted on GitHub release assets."
            />
            <Button asChild variant="ghost">
              <a href={releaseEvidence.releaseUrl} rel="noreferrer" target="_blank">
                Open release
                <ExternalLinkIcon data-icon="inline-end" />
              </a>
            </Button>
            <Button asChild variant="ghost">
              <a href="#verify">
                Verify checksums
                <ArrowRightIcon data-icon="inline-end" />
              </a>
            </Button>
          </div>

          <div className="grid gap-4 md:hidden">
            {releaseArtifacts.map((artifact) => (
              <DownloadArtifactCard key={artifact.arch} artifact={artifact} />
            ))}
            <div className="rounded-lg border bg-card p-4 text-sm text-muted-foreground">
              <p>Large OS media is served from GitHub release assets.</p>
              <p>
                Release:{" "}
                <a
                  className="font-medium text-foreground underline-offset-4 hover:underline"
                  href={releaseEvidence.releaseUrl}
                  rel="noreferrer"
                  target="_blank"
                >
                  {releaseEvidence.releaseTag}
                </a>
              </p>
            </div>
          </div>

          <Card className="hidden md:block" data-gsap="reveal">
            <CardContent className="px-0">
              <Table className="min-w-[1120px]">
                <TableHeader>
                  <TableRow>
                    <TableHead>Architecture</TableHead>
                    <TableHead>Status</TableHead>
                    <TableHead>Media</TableHead>
                    <TableHead>SHA256</TableHead>
                    <TableHead>Size</TableHead>
                    <TableHead>Built</TableHead>
                    <TableHead className="text-right">Download</TableHead>
                  </TableRow>
                </TableHeader>
                <TableBody>
                  {releaseArtifacts.map((artifact) => (
                    <TableRow key={artifact.arch}>
                      <TableCell className="min-w-[220px] whitespace-normal">
                        <div className="flex flex-col gap-1">
                          <span className="font-medium">{artifact.label}</span>
                          <span className="text-xs leading-5 text-muted-foreground">
                            {artifact.cpu}
                          </span>
                        </div>
                      </TableCell>
                      <TableCell className="min-w-[260px] whitespace-normal">
                        <div className="flex flex-col gap-2">
                          <Badge variant="secondary" className="w-fit">
                            Download ready
                          </Badge>
                          <ul className="flex flex-col gap-1 text-xs leading-5 text-muted-foreground">
                            {artifact.notes.map((note) => (
                              <li key={note}>{note}</li>
                            ))}
                          </ul>
                        </div>
                      </TableCell>
                      <TableCell className="whitespace-normal">
                        <div className="flex flex-col gap-1">
                          <code className="text-xs">{artifact.isoName}</code>
                          <code className="text-xs text-muted-foreground">
                            {artifact.compressedName}
                          </code>
                        </div>
                      </TableCell>
                      <TableCell className="max-w-[240px] whitespace-normal">
                        <code className="break-all text-xs">{artifact.sha256}</code>
                      </TableCell>
                      <TableCell>
                        <div className="flex flex-col gap-1 text-sm">
                          <span>{formatBytes(artifact.rawSizeBytes)} ISO</span>
                          <span className="text-xs text-muted-foreground">
                            {formatBytes(artifact.compressedSizeBytes)} download
                          </span>
                        </div>
                      </TableCell>
                      <TableCell>{new Date(artifact.builtOn).toISOString().slice(0, 10)}</TableCell>
                      <TableCell className="text-right">
                        <div className="flex flex-col items-end gap-2">
                          {artifact.downloadParts.map((part, index) => (
                            <Button key={part.filename} asChild variant="outline" size="sm">
                              <a href={part.url} rel="noreferrer" target="_blank">
                                Part {String(index).padStart(2, "0")}
                                <ArrowDownToLineIcon data-icon="inline-end" />
                              </a>
                            </Button>
                          ))}
                          <Button asChild variant="ghost" size="sm">
                            <a href={artifact.partsSha256Url} rel="noreferrer" target="_blank">
                              Checksums
                              <ExternalLinkIcon data-icon="inline-end" />
                            </a>
                          </Button>
                        </div>
                      </TableCell>
                    </TableRow>
                  ))}
                </TableBody>
              </Table>
            </CardContent>
            <CardFooter className="flex flex-col items-start gap-3 border-t text-sm text-muted-foreground sm:flex-row sm:items-center sm:justify-between">
              <span>Large OS media is served from GitHub release assets.</span>
              <a
                className="font-medium text-foreground underline-offset-4 hover:underline"
                href={releaseEvidence.releaseRunUrl}
                rel="noreferrer"
                target="_blank"
              >
                Build details
              </a>
            </CardFooter>
          </Card>
        </div>
      </section>

      <section id="containers" className="scroll-mt-20 border-b bg-background">
        <div className="mx-auto flex w-full max-w-7xl flex-col gap-8 px-4 py-12 sm:px-6 lg:px-8">
          <div className="flex flex-col justify-between gap-4 lg:flex-row lg:items-end">
            <SectionHeading
              title="Container images"
              description="Use the bootc container image to inspect, verify, or build from Goblins OS without writing installer media to hardware."
            />
            <Button asChild variant="ghost">
              <a href={releaseEvidence.releaseRunUrl} rel="noreferrer" target="_blank">
                Build details
                <ExternalLinkIcon data-icon="inline-end" />
              </a>
            </Button>
          </div>

          <div className="grid gap-4 lg:grid-cols-2">
            {containerImages.map((image) => (
              <ContainerImageCard key={image.arch} image={image} />
            ))}
          </div>

          <Alert data-gsap="reveal">
            <BoxIcon aria-hidden="true" />
            <AlertTitle>Containers are not a desktop VM</AlertTitle>
            <AlertDescription>
              Container images are for inspection, automation, and derived
              builds. Use the ISO when you need the graphical desktop installer.
            </AlertDescription>
          </Alert>
        </div>
      </section>

      <section id="install" className="scroll-mt-20 border-b bg-background">
        <div className="mx-auto grid w-full max-w-7xl gap-8 px-4 py-12 sm:px-6 lg:grid-cols-[1fr_0.42fr] lg:px-8">
          <div className="flex flex-col gap-8">
            <SectionHeading
              title="Install Goblins OS"
              description="The installer writes an operating system. Back up first, choose the architecture intentionally, and verify the media."
            />
            <div className="grid gap-4 md:grid-cols-3">
              {installSteps.map((step, index) => (
                <div key={step.title} className="flex gap-4 border-t pt-5" data-gsap="reveal">
                  <span className="flex size-8 shrink-0 items-center justify-center rounded-full bg-primary text-sm font-semibold text-primary-foreground">
                    {index + 1}
                  </span>
                  <div className="flex flex-col gap-2">
                    <h3 className="font-semibold">{step.title}</h3>
                    <p className="text-sm leading-6 text-muted-foreground">{step.body}</p>
                  </div>
                </div>
              ))}
            </div>
          </div>

          <Alert className="h-fit" data-gsap="reveal">
            <ShieldCheckIcon aria-hidden="true" />
            <AlertTitle>Install guardrails</AlertTitle>
            <AlertDescription>
              <ul className="flex flex-col gap-2">
                <li>Arm and x86_64 use separate installer media.</li>
                <li>Dual boot uses advanced storage and preserved partitions.</li>
                <li>Whole-disk erase requires an explicit blank-disk decision.</li>
              </ul>
            </AlertDescription>
          </Alert>
        </div>
      </section>

      <section id="verify" className="scroll-mt-20 border-b bg-muted/35">
        <div className="mx-auto grid w-full min-w-0 max-w-7xl gap-8 px-4 py-12 sm:px-6 lg:grid-cols-[0.72fr_0.28fr] lg:px-8">
          <div className="flex min-w-0 flex-col gap-6">
            <SectionHeading
              title="Verify your download"
              description="Download both parts for your architecture, verify the split files, reassemble the compressed ISO, decompress it, and verify the final ISO."
            />
            <Tabs defaultValue="macos-linux" className="w-full min-w-0">
              <TabsList className="w-full justify-start overflow-x-auto sm:w-fit">
                <TabsTrigger value="macos-linux">macOS / Linux</TabsTrigger>
                <TabsTrigger value="windows">Windows</TabsTrigger>
              </TabsList>
              <TabsContent value="macos-linux" className="min-w-0">
                <Card className="min-w-0">
                  <CardHeader>
                    <CardTitle>Reassemble and verify</CardTitle>
                    <CardDescription>
                      Replace <code>&lt;arch&gt;</code> with <code>aarch64</code> or{" "}
                      <code>x86_64</code>. Requires <code>zstd</code>.
                    </CardDescription>
                  </CardHeader>
                  <CardContent className="min-w-0">
                    <pre className="max-w-full whitespace-pre-wrap break-words rounded-md bg-muted p-4 text-sm leading-6">
                      <code>{`shasum -a 256 -c goblins-os-<arch>.iso.zst.parts.sha256
cat goblins-os-<arch>.iso.zst.part-* > goblins-os-<arch>.iso.zst
shasum -a 256 -c goblins-os-<arch>.iso.zst.sha256
zstd -d --long=31 goblins-os-<arch>.iso.zst
shasum -a 256 -c goblins-os-<arch>.iso.sha256`}</code>
                    </pre>
                  </CardContent>
                </Card>
              </TabsContent>
              <TabsContent value="windows" className="min-w-0">
                <Card className="min-w-0">
                  <CardHeader>
                    <CardTitle>PowerShell checks</CardTitle>
                    <CardDescription>
                      Compare each hash with the published <code>.sha256</code> files before flashing.
                    </CardDescription>
                  </CardHeader>
                  <CardContent className="min-w-0">
                    <pre className="max-w-full whitespace-pre-wrap break-words rounded-md bg-muted p-4 text-sm leading-6">
                      <code>{`Get-FileHash .\\goblins-os-<arch>.iso.zst.part-00 -Algorithm SHA256
Get-FileHash .\\goblins-os-<arch>.iso.zst.part-01 -Algorithm SHA256
copy /b goblins-os-<arch>.iso.zst.part-00+goblins-os-<arch>.iso.zst.part-01 goblins-os-<arch>.iso.zst
Get-FileHash .\\goblins-os-<arch>.iso.zst -Algorithm SHA256
zstd -d --long=31 .\\goblins-os-<arch>.iso.zst
Get-FileHash .\\goblins-os-<arch>.iso -Algorithm SHA256`}</code>
                    </pre>
                  </CardContent>
                </Card>
              </TabsContent>
            </Tabs>
          </div>

          <Card className="h-fit min-w-0" data-gsap="reveal">
            <CardHeader>
              <CardTitle>Release checksums</CardTitle>
              <CardDescription>
                Final ISO SHA256 values from the published release assets.
              </CardDescription>
            </CardHeader>
            <CardContent className="flex flex-col gap-4">
              {releaseArtifacts.map((artifact) => (
                <div key={artifact.arch} className="flex flex-col gap-2 rounded-md bg-muted p-3">
                  <span className="text-sm font-medium">{artifact.label}</span>
                  <code className="break-all text-xs">{artifact.sha256}</code>
                  <CopyButton value={artifact.sha256} label={`Copy ${artifact.arch} checksum`} />
                </div>
              ))}
            </CardContent>
          </Card>
        </div>
      </section>

      <section id="source" className="scroll-mt-20 bg-background">
        <div className="mx-auto grid w-full min-w-0 max-w-7xl gap-8 px-4 py-12 sm:px-6 lg:grid-cols-[0.8fr_1.2fr] lg:px-8">
          <div className="flex min-w-0 flex-col gap-5" data-gsap="reveal">
            <SectionHeading
              title="Source and provenance"
              description="Goblins OS source is AGPL-3.0-or-later. Forks must keep required notices and use their own branding unless they have permission to use the Goblins OS marks."
            />
            <div className="flex flex-wrap gap-3">
              <Button asChild>
                <a href={sourceUrl} rel="noreferrer" target="_blank">
                  GitHub repository
                  <ExternalLinkIcon data-icon="inline-end" />
                </a>
              </Button>
              <Button asChild variant="outline">
                <a href={`${sourceUrl}/blob/main/LICENSE`} rel="noreferrer" target="_blank">
                  License
                  <ArrowUpRightIcon data-icon="inline-end" />
                </a>
              </Button>
              <Button asChild variant="outline">
                <a href={`${sourceUrl}/blob/main/NOTICE`} rel="noreferrer" target="_blank">
                  Notice
                  <ArrowUpRightIcon data-icon="inline-end" />
                </a>
              </Button>
            </div>
          </div>

          <div className="grid min-w-0 gap-4 md:grid-cols-3" data-gsap="reveal">
            <EvidenceCard
              icon={FileCheck2Icon}
              title="Release process"
              href={`${sourceUrl}/blob/main/SHIP.md`}
              body="Build, verification, artifact, and signoff rules."
            />
            <EvidenceCard
              icon={HardDriveIcon}
              title="Architecture matrix"
              href={`${sourceUrl}/blob/main/os/release/architectures.toml`}
              body="Expected ISO, checksum, and manifest paths."
            />
            <EvidenceCard
              icon={ShieldCheckIcon}
              title="Marks policy"
              href={`${sourceUrl}/blob/main/TRADEMARKS.md`}
              body="Forks can use the source, but not the Goblins OS identity."
            />
          </div>
        </div>
        <Separator />
        <footer className="mx-auto flex w-full max-w-7xl flex-col gap-4 px-4 py-6 text-sm text-muted-foreground sm:px-6 md:flex-row md:items-center md:justify-between lg:px-8">
          <span>Goblins OS</span>
          <span>AGPL-3.0-or-later source. Goblins OS marks reserved.</span>
        </footer>
      </section>
    </main>
  );
}

function DownloadArtifactCard({ artifact }: { artifact: (typeof releaseArtifacts)[number] }) {
  return (
    <Card data-gsap="reveal">
      <CardHeader>
        <div className="flex items-start justify-between gap-3">
          <div className="flex flex-col gap-1">
            <CardTitle>{artifact.label}</CardTitle>
            <CardDescription>{artifact.cpu}</CardDescription>
          </div>
          <Badge variant="secondary" className="shrink-0">
            Download ready
          </Badge>
        </div>
      </CardHeader>
      <CardContent className="flex min-w-0 flex-col gap-5">
        <div className="grid gap-3 text-sm">
          <MetadataRow label="ISO">
            <code className="break-all text-xs">{artifact.isoName}</code>
          </MetadataRow>
          <MetadataRow label="Download">
            <code className="break-all text-xs">{artifact.compressedName}</code>
          </MetadataRow>
          <MetadataRow label="SHA256">
            <code className="break-all text-xs">{artifact.sha256}</code>
          </MetadataRow>
          <MetadataRow label="Size">
            {formatBytes(artifact.rawSizeBytes)} ISO ·{" "}
            {formatBytes(artifact.compressedSizeBytes)} download
          </MetadataRow>
          <MetadataRow label="Built">
            {new Date(artifact.builtOn).toISOString().slice(0, 10)}
          </MetadataRow>
        </div>
        <div className="rounded-lg border bg-muted/55 p-3">
          <p className="mb-2 text-sm font-medium">Release notes</p>
          <ul className="flex flex-col gap-1 text-xs leading-5 text-muted-foreground">
            {artifact.notes.map((note) => (
              <li key={note}>{note}</li>
            ))}
          </ul>
        </div>
        <div className="grid gap-2">
          {artifact.downloadParts.map((part, index) => (
            <Button key={part.filename} asChild variant="outline" className="w-full">
              <a href={part.url} rel="noreferrer" target="_blank">
                Download part {String(index).padStart(2, "0")}
                <ArrowDownToLineIcon data-icon="inline-end" />
              </a>
            </Button>
          ))}
        </div>
      </CardContent>
      <CardFooter className="grid gap-2 sm:grid-cols-2">
        <Button asChild variant="ghost" className="w-full">
          <a href={artifact.partsSha256Url} rel="noreferrer" target="_blank">
            Part hashes
            <ExternalLinkIcon data-icon="inline-end" />
          </a>
        </Button>
        <Button asChild variant="ghost" className="w-full">
          <a href={artifact.manifestUrl} rel="noreferrer" target="_blank">
            Manifest
            <ExternalLinkIcon data-icon="inline-end" />
          </a>
        </Button>
      </CardFooter>
    </Card>
  );
}

function ContainerImageCard({ image }: { image: ContainerImage }) {
  return (
    <Card data-gsap="reveal">
      <CardHeader>
        <div className="flex items-start justify-between gap-3">
          <div className="flex min-w-0 flex-col gap-2">
            <CardTitle>{image.label}</CardTitle>
            <CardDescription>{image.platform}</CardDescription>
          </div>
          <Badge variant="secondary" className="shrink-0">
            {image.status === "public" ? "Public pull" : "Registry pending"}
          </Badge>
        </div>
      </CardHeader>
      <CardContent className="flex flex-col gap-5">
        <MetadataRow label="Image">
          <code className="break-all text-xs">{image.image}</code>
        </MetadataRow>
        <div className="grid gap-3">
          <CommandBlock label="Docker pull" command={image.pullCommand} />
          <CommandBlock label="Docker verify" command={image.verifyCommand} />
          <CommandBlock label="Podman pull" command={image.podmanPullCommand} />
          <CommandBlock label="Podman verify" command={image.podmanVerifyCommand} />
        </div>
        <p className="text-sm leading-6 text-muted-foreground">{image.note}</p>
      </CardContent>
      <CardFooter className="flex flex-col items-stretch gap-2 sm:flex-row sm:items-center sm:justify-between">
        <Button asChild variant="ghost">
          <a href={image.sourceManifestUrl} rel="noreferrer" target="_blank">
            Source manifest
            <ExternalLinkIcon data-icon="inline-end" />
          </a>
        </Button>
        <CopyButton value={image.pullCommand} label="Copy Docker pull command" />
        <CopyButton value={image.podmanPullCommand} label="Copy Podman pull command" />
      </CardFooter>
    </Card>
  );
}

function CommandBlock({ label, command }: { label: string; command: string }) {
  return (
    <div className="flex flex-col gap-2">
      <span className="text-sm font-medium">{label}</span>
      <pre className="max-w-full whitespace-pre-wrap break-words rounded-md bg-muted p-3 text-xs leading-5">
        <code>{command}</code>
      </pre>
    </div>
  );
}

function MetadataRow({
  label,
  children,
}: {
  label: string;
  children: React.ReactNode;
}) {
  return (
    <div className="grid grid-cols-[92px_1fr] gap-3">
      <span className="text-muted-foreground">{label}</span>
      <span className="min-w-0">{children}</span>
    </div>
  );
}

function SiteHeader() {
  return (
    <header className="sticky top-0 z-40 border-b bg-background/90 backdrop-blur">
      <div className="mx-auto flex h-16 w-full max-w-7xl items-center justify-between px-4 sm:px-6 lg:px-8">
        <a href="#" className="flex items-center gap-3 font-semibold" aria-label="Goblins OS home">
          <span className="flex size-8 items-center justify-center rounded-md bg-foreground text-sm text-background">
            G
          </span>
          <span>Goblins OS</span>
        </a>
        <nav className="hidden items-center gap-7 text-sm text-muted-foreground md:flex">
          <a className="transition-colors hover:text-foreground" href="#features">
            Features
          </a>
          <a className="transition-colors hover:text-foreground" href="#screenshots">
            Screenshots
          </a>
          <a className="transition-colors hover:text-foreground" href="#downloads">
            Downloads
          </a>
          <a className="transition-colors hover:text-foreground" href="#containers">
            Containers
          </a>
          <a className="transition-colors hover:text-foreground" href="#install">
            Install
          </a>
          <a className="transition-colors hover:text-foreground" href="#source">
            Source
          </a>
        </nav>
        <Button asChild variant="outline" size="sm">
          <a href="#downloads">Check downloads</a>
        </Button>
      </div>
    </header>
  );
}

function SectionHeading({
  title,
  description,
}: {
  title: string;
  description: string;
}) {
  return (
    <div className="flex max-w-2xl flex-col gap-2" data-gsap="reveal">
      <h2 className="text-3xl font-semibold tracking-normal text-balance sm:text-4xl">
        {title}
      </h2>
      <p className="text-sm leading-6 text-muted-foreground sm:text-base">
        {description}
      </p>
    </div>
  );
}

function ProofPoint({ children }: { children: React.ReactNode }) {
  return (
    <span className="flex items-center gap-2">
      <CheckCircle2Icon className="text-primary" aria-hidden="true" />
      {children}
    </span>
  );
}

function EvidenceCard({
  icon: Icon,
  title,
  body,
  href,
}: {
  icon: typeof FileCheck2Icon;
  title: string;
  body: string;
  href: string;
}) {
  return (
    <Card>
      <CardHeader>
        <Icon className="text-primary" aria-hidden="true" />
        <CardTitle>{title}</CardTitle>
        <CardDescription>{body}</CardDescription>
      </CardHeader>
      <CardFooter>
        <Button asChild variant="ghost" size="sm">
          <a href={href} rel="noreferrer" target="_blank">
            Open
            <ArrowUpRightIcon data-icon="inline-end" />
          </a>
        </Button>
      </CardFooter>
    </Card>
  );
}
