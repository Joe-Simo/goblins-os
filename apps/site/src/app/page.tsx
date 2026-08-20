import Image from "next/image";
import {
  ArrowDownIcon,
  ArrowRightIcon,
  ArrowUpRightIcon,
  CheckIcon,
  Code2Icon,
  DownloadIcon,
  EyeIcon,
  FingerprintIcon,
  GitForkIcon,
  HardDriveIcon,
  LockKeyholeIcon,
  MonitorIcon,
  PackageCheckIcon,
  RotateCcwIcon,
  SparklesIcon,
  TerminalSquareIcon,
  WandSparklesIcon,
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
  releaseArtifacts,
  releaseEvidence,
} from "@/lib/release-data";
import { screenshots } from "@/lib/site-assets";

const sourceUrl = "https://github.com/Joe-Simo/goblins-os";
const issuesUrl = `${sourceUrl}/issues`;

const buildHighlights = [
  {
    marker: "01",
    title: "Start with an idea.",
    body: "Tell Build Studio what you want to make in your own words. A real project takes shape on your machine.",
    icon: SparklesIcon,
  },
  {
    marker: "02",
    title: "Stay in the loop.",
    body: "Open the files, read the diff, follow the build, and roll back a checkpoint whenever you want.",
    icon: EyeIcon,
  },
  {
    marker: "03",
    title: "Make it yours.",
    body: "Preview the result, keep editing, export the source, or package a supported static app to take with you.",
    icon: Code2Icon,
  },
];

const systemHighlights = [
  {
    label: "Updates",
    title: "Move forward. Or roll back.",
    body: "Check, download, and apply image updates from Settings. If the next boot is not right, return to the previous deployment.",
    icon: RotateCcwIcon,
  },
  {
    label: "Recovery",
    title: "Bring back the file you miss.",
    body: "On eligible fresh installs, browse earlier snapshots and recover a file to a new destination without overwriting what is there.",
    icon: HardDriveIcon,
  },
  {
    label: "Your space",
    title: "Set up the desktop your way.",
    body: "Arrange displays, connect Bluetooth devices, tune shortcuts and gestures, and manage network settings from native controls.",
    icon: MonitorIcon,
  },
  {
    label: "Your account",
    title: "Your keys stay yours.",
    body: "Goblins OS does not ship with a maintainer API key. Sign in for yourself, on your machine, when you choose to use AI features.",
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
  const home = screenshot("Home");
  const buildStudio = screenshot("Build Studio");
  const workspace = screenshot("Workspace Overview");
  const installer = screenshot("Installer");

  if (!release || !containerImages[0]) {
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
      <a className="skip-link" href="#build">Skip to Build Studio</a>
      <MotionReveal />
      <SiteHeader />

      <section className="hero" aria-labelledby="hero-title">
        <div className="hero__grid" aria-hidden="true" />
        <div className="hero__inner">
          <div className="hero__copy">
            <div className="hero__eyebrow" data-hero-reveal>
              <Image src="/favicon.svg" alt="" width={24} height={24} aria-hidden="true" />
              <span>Goblins OS 0.2 preview</span>
              <span className="status-pulse" aria-hidden="true" />
              <span>Available now</span>
            </div>
            <h1 id="hero-title" className="hero__title" data-hero-reveal>
              <span>Goblins</span>
              <span className="hero__title-line">
                <em>OS</em>
                <small>Imagine it. Build it. Keep it.</small>
              </span>
            </h1>
            <div className="hero__intro" data-hero-reveal>
              <p>
                The open Linux desktop that helps turn your ideas into working
                software—right on your machine.
              </p>
              <div className="hero__actions">
                <Button asChild size="lg" className="button--ink">
                  <a href="#download">
                    Download the preview
                    <ArrowDownIcon aria-hidden="true" />
                  </a>
                </Button>
                <Button asChild size="lg" variant="outline" className="button--ghost">
                  <a href="#build">
                    See what it can do
                    <ArrowRightIcon aria-hidden="true" />
                  </a>
                </Button>
              </div>
            </div>
          </div>

          <div className="hero__media" data-hero-reveal>
            <HeroReel />
            <div className="hero__media-note">
              <span>Goblins OS in motion</span>
              <span>From idea to working project</span>
            </div>
          </div>
        </div>
        <a className="hero__scroll" href="#story" aria-label="Discover Goblins OS">
          <span>Discover Goblins OS</span>
          <ArrowDownIcon aria-hidden="true" />
        </a>
      </section>

      <div className="signal-strip" aria-label="What you can do with Goblins OS">
        <div className="signal-strip__track">
          <SignalItem icon={WandSparklesIcon}>Describe it</SignalItem>
          <SignalItem icon={Code2Icon}>Build it</SignalItem>
          <SignalItem icon={EyeIcon}>Inspect it</SignalItem>
          <SignalItem icon={PackageCheckIcon}>Take it with you</SignalItem>
          <SignalItem icon={WandSparklesIcon} ariaHidden>Describe it</SignalItem>
          <SignalItem icon={Code2Icon} ariaHidden>Build it</SignalItem>
          <SignalItem icon={EyeIcon} ariaHidden>Inspect it</SignalItem>
          <SignalItem icon={PackageCheckIcon} ariaHidden>Take it with you</SignalItem>
        </div>
      </div>

      <section id="story" className="manifesto section-pad">
        <div className="manifesto__content" data-gsap="reveal">
          <p className="manifesto__lead">A computer should invite you to make something.</p>
          <p className="manifesto__statement">
            Your idea deserves more than another chat window.
          </p>
          <div className="manifesto__aside">
            <span className="eyebrow">Meet your new workshop</span>
            <p>
              Goblins OS brings the conversation, the project, and the desktop
              together—so you can move from “what if?” to “it works.”
            </p>
          </div>
        </div>
      </section>

      <section id="build" className="experience section-pad" aria-labelledby="build-title">
        <div className="experience__heading" data-gsap="reveal">
          <h2 id="build-title">Say what you want to make. See it take shape.</h2>
          <p>
            Build Studio turns a conversation into a project you can open,
            understand, run, and keep. You are never locked out of your own work.
          </p>
        </div>

        <div className="experience__steps">
          {buildHighlights.map((highlight) => (
            <article className="experience-step" key={highlight.marker} data-gsap="reveal">
              <span className="experience-step__marker">{highlight.marker}</span>
              <highlight.icon aria-hidden="true" />
              <h3>{highlight.title}</h3>
              <p>{highlight.body}</p>
            </article>
          ))}
        </div>

        <figure className="product-window product-window--wide" data-gsap="reveal">
          <WindowBar title="Build Studio" meta="Your project" />
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
            <span>Conversation, files, changes, and results—together.</span>
          </figcaption>
        </figure>
      </section>

      <section id="desktop" className="workspace section-pad" aria-labelledby="desktop-title">
        <div className="workspace__copy" data-gsap="reveal">
          <span className="eyebrow">A desktop that keeps up</span>
          <h2 id="desktop-title">Everything you are making. Right where you left it.</h2>
          <p>
            Move between projects, windows, settings, and workspaces without
            losing your train of thought. Goblins OS is a complete desktop—not
            a browser wrapped around a prompt.
          </p>
          <Button asChild variant="secondary" size="lg">
            <a href="#control">
              Explore the desktop
              <ArrowDownIcon aria-hidden="true" />
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
            <span>Your workspace</span>
            <span>Windows and workspaces, at a glance.</span>
          </figcaption>
        </figure>
      </section>

      <section id="control" className="system section-pad" aria-labelledby="control-title">
        <div className="system__heading" data-gsap="reveal">
          <h2 id="control-title">Powerful when you need it. Forgiving when you do not.</h2>
          <p>
            The best system features are the ones that let you try something,
            change your mind, and get back to what matters.
          </p>
        </div>

        <div className="system__grid">
          {systemHighlights.map((highlight, index) => (
            <article className="system-capability" key={highlight.label} data-gsap="reveal">
              <div className="system-capability__top">
                <span>{String(index + 1).padStart(2, "0")}</span>
                <highlight.icon aria-hidden="true" />
              </div>
              <span className="eyebrow">{highlight.label}</span>
              <h3>{highlight.title}</h3>
              <p>{highlight.body}</p>
            </article>
          ))}
        </div>

        <div className="system__screens">
          <figure className="product-window product-window--home" data-gsap="reveal">
            <WindowBar title="Goblins OS" meta="Ready when you are" />
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
            <WindowBar title="Install Goblins OS" meta="Choose carefully" />
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

      <section className="open-source section-pad" aria-labelledby="source-title">
        <div className="open-source__mark" data-gsap="reveal">
          <Image src="/favicon.svg" alt="" width={120} height={120} aria-hidden="true" />
        </div>
        <div className="open-source__copy" data-gsap="reveal">
          <span className="eyebrow">Open source, on purpose</span>
          <h2 id="source-title">See how it works. Change what it becomes.</h2>
          <p>
            Goblins OS is built in public and licensed under AGPL-3.0-or-later.
            Read the code, follow the work, make a fork, or help shape what comes next.
          </p>
          <div className="open-source__actions">
            <Button asChild size="lg">
              <a href={sourceUrl} target="_blank" rel="noreferrer">
                <GitForkIcon aria-hidden="true" />
                Explore the source
              </a>
            </Button>
            <Button asChild size="lg" variant="outline">
              <a href={issuesUrl} target="_blank" rel="noreferrer">
                Share an idea
                <ArrowUpRightIcon aria-hidden="true" />
              </a>
            </Button>
          </div>
        </div>
      </section>

      <section id="download" className="download section-pad" aria-labelledby="download-title">
        <div className="download__halo" aria-hidden="true" />
        <div className="download__heading" data-gsap="reveal">
          <Badge variant="secondary">{releaseEvidence.releaseTag}</Badge>
          <h2 id="download-title">Ready to meet the goblins?</h2>
          <p>
            Try the current preview in an ARM64 UEFI virtual machine. It is made
            for exploring, showing friends, and telling us what should get better next.
          </p>
        </div>

        <div className="download__layout">
          <Card className="download-card" data-gsap="reveal">
            <CardHeader>
              <div className="download-card__icon"><DownloadIcon aria-hidden="true" /></div>
              <div>
                <span className="eyebrow">ARM64 virtual machines</span>
                <CardTitle>Download Goblins OS 0.2 preview</CardTitle>
              </div>
            </CardHeader>
            <CardContent>
              <div className="download-card__facts">
                <ReleaseFact label="Download" value={formatBytes(release.compressedSizeBytes)} />
                <ReleaseFact label="Installer" value={formatBytes(release.rawSizeBytes)} />
                <ReleaseFact label="Published" value={new Date(release.builtOn).toLocaleDateString("en-US", { month: "short", day: "numeric", year: "numeric" })} />
              </div>
              <p className="download-card__intro">
                The installer is split into two parts for GitHub. Download both,
                then use the verification guide below to put them back together.
              </p>
              <div className="download-card__parts">
                {release.downloadParts.map((part, index) => (
                  <Button asChild size="lg" key={part.filename} className="download-part">
                    <a href={part.url}>
                      <span>
                        <small>Download {index + 1} of {release.downloadParts.length}</small>
                        {part.filename}
                      </span>
                      <strong>{formatBytes(part.sizeBytes)}</strong>
                      <DownloadIcon aria-hidden="true" />
                    </a>
                  </Button>
                ))}
              </div>
              <div className="download-card__links">
                <a href="#verify">How to verify <ArrowDownIcon aria-hidden="true" /></a>
                <a href={releaseEvidence.releaseUrl} target="_blank" rel="noreferrer">Release notes <ArrowUpRightIcon aria-hidden="true" /></a>
                <a href={release.manifestUrl}>Build details <ArrowUpRightIcon aria-hidden="true" /></a>
              </div>
            </CardContent>
          </Card>

          <div className="release-ledger" data-gsap="reveal">
            <div className="release-ledger__top">
              <FingerprintIcon aria-hidden="true" />
              <span>Before you download</span>
            </div>
            <ReleaseFact label="Runs on" value="ARM64 UEFI virtual machines" />
            <ReleaseFact label="Best for" value="Exploring, demos, and feedback" />
            <ReleaseFact label="Release" value="Community preview" />
            <div className="release-ledger__digest">
              <span>Installer fingerprint · SHA-256</span>
              <code>{release.sha256}</code>
            </div>
            <Button asChild variant="outline" className="w-full">
              <a href={releaseEvidence.releaseUrl} target="_blank" rel="noreferrer">
                View the release on GitHub
                <ArrowUpRightIcon aria-hidden="true" />
              </a>
            </Button>
          </div>
        </div>

        <Alert className="download__guardrail" data-gsap="reveal">
          <PackageCheckIcon aria-hidden="true" />
          <AlertTitle>What this preview supports</AlertTitle>
          <AlertDescription>
            Run it in an ARM64 UEFI virtual machine with a disposable virtual
            disk. It is not a bare-metal Apple Silicon installer, and Intel or
            AMD computers are not supported by this preview.
          </AlertDescription>
        </Alert>
      </section>

      <section id="verify" className="verify section-pad" aria-labelledby="verify-title">
        <div className="verify__layout">
          <div className="verify__copy" data-gsap="reveal">
            <span className="eyebrow">A download you can check yourself</span>
            <h2 id="verify-title">Know exactly what you are installing.</h2>
            <p>
              Check both downloads, rebuild the installer, and confirm its
              fingerprint before you attach it to a virtual machine.
            </p>
            <div className="verify__checks">
              <span><CheckIcon aria-hidden="true" /> Check both downloads</span>
              <span><CheckIcon aria-hidden="true" /> Rebuild the installer</span>
              <span><CheckIcon aria-hidden="true" /> Confirm the final fingerprint</span>
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

      <footer className="site-footer">
        <div className="site-footer__brand">
          <Image src="/favicon.svg" alt="" width={28} height={28} aria-hidden="true" />
          <strong>Goblins OS</strong>
          <span>Imagine it. Build it. Keep it.</span>
        </div>
        <div className="site-footer__links">
          <a href="#top">Back to top</a>
          <a href={`${sourceUrl}/blob/main/NOTICE`}>Notice</a>
          <a href={`${sourceUrl}/blob/main/TRADEMARKS.md`}>Trademarks</a>
          <a href={releaseEvidence.releaseRunUrl}>Build record</a>
        </div>
        <p>
          Goblins OS is built on Fedora bootc 44. Source code is
          AGPL-3.0-or-later. Goblins OS product identity and marks are reserved.
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
        <a href="#build">Build</a>
        <a href="#desktop">Desktop</a>
        <a href="#control">Control</a>
      </nav>
      <Button asChild size="sm">
        <a href="#download">
          Download
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
  icon: typeof WandSparklesIcon;
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
