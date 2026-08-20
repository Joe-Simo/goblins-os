import Image from "next/image";
import {
  ArrowDownIcon,
  ArrowRightIcon,
  ArrowUpRightIcon,
  BoxIcon,
  CheckIcon,
  Code2Icon,
  CpuIcon,
  DownloadIcon,
  FingerprintIcon,
  GitForkIcon,
  HardDriveIcon,
  LockKeyholeIcon,
  MonitorIcon,
  PackageCheckIcon,
  RotateCcwIcon,
  ShieldCheckIcon,
  SparklesIcon,
  TerminalSquareIcon,
} from "lucide-react";
import { Alert, AlertDescription, AlertTitle } from "@/components/ui/alert";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { CopyButton } from "@/components/copy-button";
import { HeroReel } from "@/components/hero-reel";
import { MotionReveal } from "@/components/motion-reveal";
import {
  containerImages,
  formatBytes,
  historicalReleaseArtifacts,
  releaseArtifacts,
  releaseEvidence,
} from "@/lib/release-data";
import { screenshots } from "@/lib/site-assets";

const sourceUrl = "https://github.com/Joe-Simo/goblins-os";
const issuesUrl = `${sourceUrl}/issues`;

const experienceSteps = [
  {
    marker: "01",
    title: "Describe the thing.",
    body: "Start with a sentence. Build Studio turns intent into a local project while keeping the work visible and reviewable.",
    icon: SparklesIcon,
  },
  {
    marker: "02",
    title: "See every change.",
    body: "Review files, diffs, logs, and checkpoints before anything leaves your machine. Undo stays part of the workflow.",
    icon: Code2Icon,
  },
  {
    marker: "03",
    title: "Run it your way.",
    body: "Preview static apps locally, export deterministic source, or package supported projects into an OCI image layout.",
    icon: BoxIcon,
  },
];

const systemCapabilities = [
  {
    label: "System image",
    title: "Updates without losing the plot",
    body: "The public aarch64 channel supports image-based check, download, apply, reboot, and rollback flows.",
    icon: RotateCcwIcon,
  },
  {
    label: "Recovery",
    title: "A way back when work gets weird",
    body: "Eligible fresh Btrfs installs expose bounded Snapper file recovery without silently overwriting the destination.",
    icon: HardDriveIcon,
  },
  {
    label: "Desktop",
    title: "Real native surfaces",
    body: "Rust, GNOME technologies, systemd services, display controls, Bluetooth, input, gestures, and proxy settings.",
    icon: MonitorIcon,
  },
  {
    label: "Credentials",
    title: "Your account stays yours",
    body: "The image ships without a maintainer API key. Personal credentials are configured per user and kept outside the public OS image.",
    icon: LockKeyholeIcon,
  },
];

function screenshot(title: string) {
  const match = screenshots.find((candidate) => candidate.title === title);
  if (!match) {
    throw new Error(`Missing website screenshot: ${title}`);
  }
  return match;
}

export default function Home() {
  const release = releaseArtifacts[0];
  const image = containerImages[0];
  const home = screenshot("Home");
  const buildStudio = screenshot("Build Studio");
  const workspace = screenshot("Workspace Overview");
  const installer = screenshot("Installer");

  if (!release || !image) {
    throw new Error("The current aarch64 release data is incomplete.");
  }

  const macLinuxVerificationCommand = [
    "(",
    "set -eu",
    "if command -v sha256sum >/dev/null 2>&1; then",
    "  verify_sha256() { sha256sum -c -; }",
    "elif command -v shasum >/dev/null 2>&1; then",
    "  verify_sha256() { shasum -a 256 -c -; }",
    "else",
    "  echo 'Install sha256sum or shasum before continuing.' >&2; exit 1",
    "fi",
    ...release.downloadParts.map(
      (part) =>
        `printf '%s  %s\\n' '${part.sha256}' '${part.filename}' | verify_sha256`,
    ),
    `cat ${release.downloadParts.map(({ filename }) => `'${filename}'`).join(" ")} > '${release.compressedName}'`,
    `printf '%s  %s\\n' '${release.compressedSha256}' '${release.compressedName}' | verify_sha256`,
    `zstd -d --long=31 --force '${release.compressedName}'`,
    `printf '%s  %s\\n' '${release.sha256}' '${release.isoName}' | verify_sha256`,
    ")",
  ].join("\n");

  const windowsVerificationCommand = [
    "& {",
    "  $ErrorActionPreference = 'Stop'",
    "  Set-StrictMode -Version Latest",
    ...release.downloadParts.map(
      (part) =>
        `  if ((Get-FileHash '.\\${part.filename}' -Algorithm SHA256).Hash.ToLowerInvariant() -ne '${part.sha256}') { throw 'Checksum mismatch: ${part.filename}' }`,
    ),
    `  cmd /c copy /b /y ${release.downloadParts.map(({ filename }) => filename).join("+")} ${release.compressedName}`,
    "  if ($LASTEXITCODE -ne 0) { throw 'Could not reassemble the compressed ISO.' }",
    `  if ((Get-FileHash '.\\${release.compressedName}' -Algorithm SHA256).Hash.ToLowerInvariant() -ne '${release.compressedSha256}') { throw 'Checksum mismatch: ${release.compressedName}' }`,
    `  zstd -d --long=31 --force '.\\${release.compressedName}'`,
    "  if ($LASTEXITCODE -ne 0) { throw 'Could not decompress the ISO.' }",
    `  if ((Get-FileHash '.\\${release.isoName}' -Algorithm SHA256).Hash.ToLowerInvariant() -ne '${release.sha256}') { throw 'Checksum mismatch: ${release.isoName}' }`,
    "}",
  ].join("\n");

  return (
    <main className="site-shell" id="main-content">
      <a className="skip-link" href="#experience">Skip to the experience</a>
      <MotionReveal />
      <SiteHeader />

      <section className="hero" aria-labelledby="hero-title">
        <div className="hero__grid" aria-hidden="true" />
        <div className="hero__inner">
          <div className="hero__copy">
            <div className="hero__eyebrow" data-hero-reveal>
              <Image src="/favicon.svg" alt="" width={24} height={24} aria-hidden="true" />
              <span>Public aarch64 preview</span>
              <span className="status-pulse" aria-hidden="true" />
              <span>Live now</span>
            </div>
            <h1 id="hero-title" className="hero__title" data-hero-reveal>
              <span>Goblins</span>
              <span className="hero__title-line">
                <em>OS</em>
                <small>Build locally. Keep control.</small>
              </span>
            </h1>
            <div className="hero__intro" data-hero-reveal>
              <p>
                An open, AI-native Linux desktop that turns a sentence into
                software you can inspect, run, export, and own.
              </p>
              <div className="hero__actions">
                <Button asChild size="lg" className="button--ink">
                  <a href="#download">
                    Get the preview
                    <ArrowDownIcon aria-hidden="true" />
                  </a>
                </Button>
                <Button asChild size="lg" variant="outline" className="button--ghost">
                  <a href={sourceUrl} rel="noreferrer" target="_blank">
                    Explore the source
                    <ArrowUpRightIcon aria-hidden="true" />
                  </a>
                </Button>
              </div>
            </div>
          </div>

          <div className="hero__media" data-hero-reveal>
            <HeroReel />
            <div className="hero__media-note">
              <span>01 / Product tour</span>
              <span>Native desktop · local checkout</span>
            </div>
          </div>
        </div>
        <a className="hero__scroll" href="#story" aria-label="Continue to the Goblins OS story">
          <span>Scroll to enter</span>
          <ArrowDownIcon aria-hidden="true" />
        </a>
      </section>

      <div className="signal-strip" aria-label="Goblins OS release facts">
        <div className="signal-strip__track">
          <SignalItem icon={CpuIcon}>ARM64 native</SignalItem>
          <SignalItem icon={TerminalSquareIcon}>Rust desktop</SignalItem>
          <SignalItem icon={ShieldCheckIcon}>No bundled API key</SignalItem>
          <SignalItem icon={PackageCheckIcon}>Checksum-bound preview</SignalItem>
          <SignalItem icon={CpuIcon} ariaHidden>ARM64 native</SignalItem>
          <SignalItem icon={TerminalSquareIcon} ariaHidden>Rust desktop</SignalItem>
          <SignalItem icon={ShieldCheckIcon} ariaHidden>No bundled API key</SignalItem>
          <SignalItem icon={PackageCheckIcon} ariaHidden>Checksum-bound preview</SignalItem>
        </div>
      </div>

      <section id="story" className="manifesto section-pad">
        <div className="section-index" data-gsap="reveal">
          <span>00</span>
          <span>The premise</span>
        </div>
        <div className="manifesto__content" data-gsap="reveal">
          <p className="manifesto__lead">Computers should feel like possibility.</p>
          <p className="manifesto__statement">
            Goblins OS is a workshop disguised as a desktop—quiet enough to
            focus, capable enough to build, and transparent enough to trust.
          </p>
          <div className="manifesto__aside">
            <span className="eyebrow">Made for curious builders</span>
            <p>
              Describe the idea. Review the project. Run it locally. Keep the
              source and the system under your control.
            </p>
          </div>
        </div>
      </section>

      <section id="experience" className="experience section-pad" aria-labelledby="experience-title">
        <div className="section-index section-index--light" data-gsap="reveal">
          <span>01</span>
          <span>From thought to software</span>
        </div>
        <div className="experience__heading" data-gsap="reveal">
          <h2 id="experience-title">The shortest distance from idea to artifact.</h2>
          <p>
            The AI layer is not a floating chatbot. It lives inside a real build
            flow with files, logs, checkpoints, previews, and exports.
          </p>
        </div>

        <div className="experience__steps">
          {experienceSteps.map((step) => (
            <article className="experience-step" key={step.marker} data-gsap="reveal">
              <span className="experience-step__marker">{step.marker}</span>
              <step.icon aria-hidden="true" />
              <h3>{step.title}</h3>
              <p>{step.body}</p>
            </article>
          ))}
        </div>

        <figure className="product-window product-window--wide" data-gsap="reveal">
          <WindowBar title="Build Studio · New build" meta="local / main" />
          <Image
            src={buildStudio.src}
            alt={buildStudio.alt}
            width={buildStudio.width}
            height={buildStudio.height}
            sizes="(min-width: 1280px) 1200px, 94vw"
            className="product-window__image"
          />
          <figcaption>
            <span>Build Studio</span>
            <span>{buildStudio.description}</span>
          </figcaption>
        </figure>
      </section>

      <section className="workspace section-pad" aria-labelledby="workspace-title">
        <div className="workspace__copy" data-gsap="reveal">
          <div className="section-index section-index--dark">
            <span>02</span>
            <span>A desktop, not a demo</span>
          </div>
          <h2 id="workspace-title">Your whole workspace, still within reach.</h2>
          <p>
            Mission Control, native Settings, window switching, display tools,
            recovery, and the build experience live in one coherent desktop.
          </p>
          <Button asChild variant="secondary" size="lg">
            <a href="#system">
              Explore the system
              <ArrowRightIcon aria-hidden="true" />
            </a>
          </Button>
        </div>
        <figure className="workspace__frame" data-gsap="reveal">
          <Image
            src={workspace.src}
            alt={workspace.alt}
            width={workspace.width}
            height={workspace.height}
            sizes="(min-width: 1024px) 68vw, 100vw"
            className="workspace__image"
          />
          <figcaption>
            <span>Mission Control</span>
            <span>Windows, workspaces, and the native system surface.</span>
          </figcaption>
        </figure>
      </section>

      <section id="system" className="system section-pad" aria-labelledby="system-title">
        <div className="section-index" data-gsap="reveal">
          <span>03</span>
          <span>Own the machine</span>
        </div>
        <div className="system__heading" data-gsap="reveal">
          <h2 id="system-title">A playful front. A serious operating system underneath.</h2>
          <p>
            Built on Fedora bootc 44, with product identity, policy, release
            verification, and native control surfaces owned by Goblins OS.
          </p>
        </div>

        <div className="system__grid">
          {systemCapabilities.map((capability, index) => (
            <article className="system-capability" key={capability.label} data-gsap="reveal">
              <div className="system-capability__top">
                <span>{String(index + 1).padStart(2, "0")}</span>
                <capability.icon aria-hidden="true" />
              </div>
              <span className="eyebrow">{capability.label}</span>
              <h3>{capability.title}</h3>
              <p>{capability.body}</p>
            </article>
          ))}
        </div>

        <div className="system__screens">
          <figure className="product-window product-window--home" data-gsap="reveal">
            <WindowBar title="Goblins OS · Home" meta="local first" />
            <Image
              src={home.src}
              alt={home.alt}
              width={home.width}
              height={home.height}
              sizes="(min-width: 1024px) 62vw, 100vw"
              className="product-window__image"
            />
          </figure>
          <figure className="product-window product-window--installer" data-gsap="reveal">
            <WindowBar title="Installer" meta="guarded storage" />
            <Image
              src={installer.src}
              alt={installer.alt}
              width={installer.width}
              height={installer.height}
              sizes="(min-width: 1024px) 42vw, 100vw"
              className="product-window__image"
            />
          </figure>
        </div>
      </section>

      <section id="download" className="download section-pad" aria-labelledby="download-title">
        <div className="download__halo" aria-hidden="true" />
        <div className="section-index" data-gsap="reveal">
          <span>04</span>
          <span>Community preview</span>
        </div>
        <div className="download__heading" data-gsap="reveal">
          <Badge variant="secondary">{releaseEvidence.releaseTag}</Badge>
          <h2 id="download-title">Take the workshop for a spin.</h2>
          <p>
            Install the current preview in a UEFI aarch64 virtual machine. Use
            a disposable virtual disk, keep backups, and expect a few goblins.
          </p>
        </div>

        <div className="download__layout">
          <Card className="download-card" data-gsap="reveal">
            <CardHeader>
              <div className="download-card__icon"><DownloadIcon aria-hidden="true" /></div>
              <div>
                <span className="eyebrow">Split installer media</span>
                <CardTitle>{release.label}</CardTitle>
              </div>
            </CardHeader>
            <CardContent>
              <div className="download-card__facts">
                <ReleaseFact label="Compressed" value={formatBytes(release.compressedSizeBytes)} />
                <ReleaseFact label="Raw ISO" value={formatBytes(release.rawSizeBytes)} />
                <ReleaseFact label="Built" value={new Date(release.builtOn).toLocaleDateString("en-US", { month: "short", day: "numeric", year: "numeric" })} />
              </div>
              <div className="download-card__parts">
                {release.downloadParts.map((part, index) => (
                  <Button asChild size="lg" key={part.filename} className="download-part">
                    <a href={part.url}>
                      <span>
                        <small>Part {String(index + 1).padStart(2, "0")}</small>
                        {part.filename}
                      </span>
                      <strong>{formatBytes(part.sizeBytes)}</strong>
                      <DownloadIcon aria-hidden="true" />
                    </a>
                  </Button>
                ))}
              </div>
              <div className="download-card__links">
                <a href={release.partsSha256Url}>Part checksums <ArrowUpRightIcon aria-hidden="true" /></a>
                <a href={release.manifestUrl}>Build manifest <ArrowUpRightIcon aria-hidden="true" /></a>
                <a href={release.evidenceUrl}>Release evidence <ArrowUpRightIcon aria-hidden="true" /></a>
              </div>
            </CardContent>
          </Card>

          <div className="release-ledger" data-gsap="reveal">
            <div className="release-ledger__top">
              <FingerprintIcon aria-hidden="true" />
              <span>Exact release ledger</span>
            </div>
            <ReleaseFact label="Architecture" value="aarch64 / ARM64" />
            <ReleaseFact label="Target" value="UEFI virtual machine" />
            <ReleaseFact label="Channel" value={image.image} />
            <ReleaseFact label="Status" value="Installable community preview" />
            <div className="release-ledger__digest">
              <span>ISO SHA256</span>
              <code>{release.sha256}</code>
            </div>
            <Button asChild variant="outline" className="w-full">
              <a href={releaseEvidence.releaseUrl} target="_blank" rel="noreferrer">
                Open the GitHub release
                <ArrowUpRightIcon aria-hidden="true" />
              </a>
            </Button>
          </div>
        </div>

        <Alert className="download__guardrail" data-gsap="reveal">
          <ShieldCheckIcon aria-hidden="true" />
          <AlertTitle>Preview boundaries</AlertTitle>
          <AlertDescription>
            This is not the formally signed stable release. Apple Silicon is a
            convenient Arm VM host, not a bare-metal install target. Intel and
            AMD x86_64 systems are not supported by this preview.
          </AlertDescription>
        </Alert>
      </section>

      <section id="verify" className="verify section-pad" aria-labelledby="verify-title">
        <div className="section-index section-index--dark" data-gsap="reveal">
          <span>05</span>
          <span>Trust, but verify</span>
        </div>
        <div className="verify__layout">
          <div className="verify__copy" data-gsap="reveal">
            <h2 id="verify-title">You should not have to take the website&apos;s word for it.</h2>
            <p>
              Verify every split part, reassemble the compressed image, then
              verify the final ISO before attaching it to a virtual machine.
            </p>
            <div className="verify__checks">
              <span><CheckIcon aria-hidden="true" /> Split parts</span>
              <span><CheckIcon aria-hidden="true" /> Compressed image</span>
              <span><CheckIcon aria-hidden="true" /> Final installer ISO</span>
            </div>
          </div>

          <Tabs defaultValue="unix" className="verify-console" data-gsap="reveal">
            <div className="verify-console__bar">
              <TabsList>
                <TabsTrigger value="unix">macOS / Linux</TabsTrigger>
                <TabsTrigger value="windows">Windows</TabsTrigger>
              </TabsList>
            </div>
            <TabsContent value="unix">
              <CommandBlock label="Terminal" command={macLinuxVerificationCommand} />
            </TabsContent>
            <TabsContent value="windows">
              <CommandBlock label="PowerShell" command={windowsVerificationCommand} />
            </TabsContent>
          </Tabs>
        </div>
      </section>

      <section className="open-source section-pad" aria-labelledby="source-title">
        <div className="open-source__mark" data-gsap="reveal">
          <Image src="/favicon.svg" alt="" width={120} height={120} aria-hidden="true" />
        </div>
        <div className="open-source__copy" data-gsap="reveal">
          <span className="eyebrow">Open source, with a point of view</span>
          <h2 id="source-title">Read the code. Break the build. Tell us what got weird.</h2>
          <p>
            Goblins OS is licensed under AGPL-3.0-or-later. The project keeps
            source, release evidence, issue tracking, and the public build story
            in the open.
          </p>
          <div className="open-source__actions">
            <Button asChild size="lg">
              <a href={sourceUrl} target="_blank" rel="noreferrer">
                <GitForkIcon aria-hidden="true" />
                Browse the repository
              </a>
            </Button>
            <Button asChild size="lg" variant="outline">
              <a href={issuesUrl} target="_blank" rel="noreferrer">
                Share feedback
                <ArrowUpRightIcon aria-hidden="true" />
              </a>
            </Button>
          </div>
        </div>
      </section>

      <section className="archive section-pad" aria-labelledby="archive-title">
        <div data-gsap="reveal">
          <span className="eyebrow">Immutable archive</span>
          <h2 id="archive-title">Earlier experiments stay on the record.</h2>
        </div>
        <div className="archive__list">
          {historicalReleaseArtifacts.map((artifact) => (
            <a
              className="archive-row"
              href={artifact.releaseUrl}
              key={artifact.arch}
              target="_blank"
              rel="noreferrer"
              data-gsap="reveal"
            >
              <span>{artifact.arch}</span>
              <strong>{artifact.label}</strong>
              <small>{artifact.releaseTag}</small>
              <ArrowUpRightIcon aria-hidden="true" />
            </a>
          ))}
        </div>
      </section>

      <footer className="site-footer">
        <div className="site-footer__brand">
          <Image src="/favicon.svg" alt="" width={28} height={28} aria-hidden="true" />
          <strong>Goblins OS</strong>
          <span>Build locally. Keep control.</span>
        </div>
        <div className="site-footer__links">
          <a href="#top">Back to top</a>
          <a href={`${sourceUrl}/blob/main/NOTICE`}>Notice</a>
          <a href={`${sourceUrl}/blob/main/TRADEMARKS.md`}>Trademarks</a>
          <a href={releaseEvidence.releaseRunUrl}>Build run</a>
        </div>
        <p>
          Built on Fedora bootc 44. Goblins OS product identity and marks are
          reserved. Source code is AGPL-3.0-or-later.
        </p>
      </footer>
    </main>
  );
}

function SiteHeader() {
  return (
    <header className="site-header" id="top">
      <a className="site-header__brand" href="#top" aria-label="Goblins OS home">
        <Image src="/favicon.svg" alt="" width={24} height={24} aria-hidden="true" />
        <span>Goblins OS</span>
      </a>
      <nav aria-label="Primary navigation">
        <a href="#experience">Experience</a>
        <a href="#system">System</a>
        <a href="#verify">Verify</a>
      </nav>
      <Button asChild size="sm">
        <a href="#download">
          Get preview
          <ArrowRightIcon aria-hidden="true" />
        </a>
      </Button>
    </header>
  );
}

function SignalItem({
  children,
  icon: Icon,
  ariaHidden = false,
}: {
  children: React.ReactNode;
  icon: typeof CpuIcon;
  ariaHidden?: boolean;
}) {
  return (
    <span className="signal-item" aria-hidden={ariaHidden || undefined}>
      <Icon aria-hidden="true" />
      {children}
    </span>
  );
}

function WindowBar({ title, meta }: { title: string; meta: string }) {
  return (
    <div className="window-bar" aria-hidden="true">
      <span className="window-dot window-dot--red" />
      <span className="window-dot window-dot--yellow" />
      <span className="window-dot window-dot--green" />
      <span className="window-title">{title}</span>
      <span className="window-live">{meta}</span>
    </div>
  );
}

function ReleaseFact({ label, value }: { label: string; value: string }) {
  return (
    <div className="release-fact">
      <span>{label}</span>
      <strong>{value}</strong>
    </div>
  );
}

function CommandBlock({ label, command }: { label: string; command: string }) {
  return (
    <div className="command-block">
      <div className="command-block__header">
        <span><TerminalSquareIcon aria-hidden="true" />{label}</span>
        <CopyButton value={command} label={`Copy ${label} verification command`} />
      </div>
      <pre tabIndex={0}><code>{command}</code></pre>
    </div>
  );
}
