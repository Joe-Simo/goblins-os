import AppKit
import CoreFoundation
import CryptoKit
import Darwin
import Foundation
import ImageIO
import Vision

private let expectedScreenshots = [
    "01-installer.png",
    "02-install-network.png",
    "03-login.png",
    "04-desktop.png",
    "05-first-boot-private-unlock.png",
    "06-onboarding.png",
    "07-home.png",
    "08-shell-home.png",
    "09-shell-dark.png",
    "10-settings.png",
    "11-settings-models.png",
    "12-settings-dark.png",
    "13-studio-before.png",
    "14-studio-running.png",
    "15-studio-app-detail.png",
    "16-built-app-open.png",
    "17-dark-motion.png",
    "18-light-motion.png",
    "19-vulkan-vkcube.png",
    "20-gamemode-active.png",
    "21-gamescope-session.png",
    "22-mangohud-overlay.png",
    "23-controller-detection.png",
    "24-audio-output.png",
    "25-install-destination.png",
    "26-install-storage-summary.png",
    "27-dual-boot-preserve-existing-os.png",
    "28-bootloader-efi-summary.png",
    "29-preview-pdf-open.png",
    "30-preview-image-open.png",
    "31-text-shortcuts-candidate-bubble-render.png",
    "32-text-shortcuts-live-ibus-runtime-render.png",
    "33-accessibility-text-scaling.png",
    "34-accessibility-high-contrast.png",
    "35-accessibility-reduced-transparency.png",
    "36-accessibility-reduced-motion.png",
    "37-accessibility-localization-expansion.png",
    "38-accessibility-orca-atspi.png",
    "39-accessibility-keyboard-focus.png",
    "40-accessibility-window-resize.png",
    "41-hosted-context-review.png",
    "42-hosted-context-review-dark.png",
]

private let canonicalWidth = 5120
private let canonicalHeight = 2880
private let maximumScreenshotBytes = 128 * 1024 * 1024
private let maximumSealBytes = 16 * 1024 * 1024
private let maximumRecognizedCharacters = 2 * 1024 * 1024

private struct Detector {
    let label: String
    let expression: NSRegularExpression

    init(_ label: String, _ pattern: String, options: NSRegularExpression.Options = []) throws {
        self.label = label
        expression = try NSRegularExpression(pattern: pattern, options: options)
    }

    func matches(_ value: String) -> Bool {
        let range = NSRange(value.startIndex..<value.endIndex, in: value)
        return expression.firstMatch(in: value, range: range) != nil
    }
}

private struct SealEntry {
    let sha256: String
    let size: Int
    let width: Int
    let height: Int
}

private func scannerError(_ code: Int, _ description: String) -> NSError {
    NSError(
        domain: "GoblinsOSVisualSecretScan",
        code: code,
        userInfo: [NSLocalizedDescriptionKey: description]
    )
}

private func makeDetectors() throws -> [Detector] {
    try [
        Detector("OpenAI API credential", #"\bsk-(?:proj-|svcacct-|admin-)?[A-Za-z0-9_-]{16,}"#),
        Detector("GitHub credential", #"\bgh[pousr]_[A-Za-z0-9]{20,}"#),
        Detector("GitHub fine-grained credential", #"\bgithub_pat_[A-Za-z0-9_]{20,}"#),
        Detector("AWS access key", #"\b(?:AKIA|ASIA)[0-9A-Z]{16}\b"#),
        Detector("private key material", #"-----BEGIN (?:[A-Z0-9 ]+ )?PRIVATE KEY-----"#),
        Detector("JWT credential", #"\beyJ[A-Za-z0-9_-]{8,}\.[A-Za-z0-9_-]{8,}\.[A-Za-z0-9_-]{8,}\b"#),
        Detector(
            "Bearer credential",
            #"\bBearer[ \t]+[A-Za-z0-9_./+=-]{16,}"#,
            options: [.caseInsensitive]
        ),
        Detector(
            "compact Bearer credential",
            #"\bBearer[A-Za-z0-9_./+=-]{20,}\b"#,
            options: [.caseInsensitive]
        ),
        Detector(
            "visible secret assignment",
            #"\b(?:api[ _-]?key|access[ _-]?token|client[ _-]?secret|password|[A-Z][A-Z0-9_]*(?:API_KEY|ACCESS_TOKEN|CLIENT_SECRET|PASSWORD|TOKEN|SECRET))[ \t]*[:=][ \t]*[\"']?[A-Za-z0-9_./+=-]{16,}"#,
            options: [.caseInsensitive]
        ),
    ]
}

private func detectorMatches(_ variants: [String], detectors: [Detector]) -> String? {
    for value in variants {
        for detector in detectors where detector.matches(value) {
            return detector.label
        }
    }
    return nil
}

private func detectionVariants(_ candidateGroups: [[String]]) throws -> [String] {
    let totalCharacters = candidateGroups.flatMap { $0 }.reduce(0) { $0 + $1.count }
    guard totalCharacters <= maximumRecognizedCharacters else {
        throw scannerError(2, "Apple Vision returned excessive recognized text")
    }
    var variants = candidateGroups.flatMap { $0 }
    let maximumRank = candidateGroups.map(\.count).max() ?? 0
    for rank in 0..<maximumRank {
        let lines = candidateGroups.compactMap { group in
            rank < group.count ? group[rank] : nil
        }
        guard !lines.isEmpty else { continue }
        let newlineJoined = lines.joined(separator: "\n")
        let spaceJoined = newlineJoined.replacingOccurrences(
            of: #"\s+"#,
            with: " ",
            options: .regularExpression
        )
        let compact = newlineJoined.replacingOccurrences(
            of: #"\s+"#,
            with: "",
            options: .regularExpression
        )
        variants.append(newlineJoined)
        variants.append(spaceJoined)
        variants.append(compact)
    }
    return variants
}

private func runDetectorSelfTest(_ detectors: [Detector]) throws {
    let positiveCases = [
        "API key: sk-proj-abcdefghijklmnopqrstuvwxyz012345",
        "token=ghp_abcdefghijklmnopqrstuvwxyz012345",
        "token=github_pat_abcdefghijklmnopqrstuvwxyz_0123456789",
        "OPENAI_ACCOUNT_CLIENT_SECRET='abcdefghijklmnopqrstuvwxyz012345'",
        "Authorization: Bearer abcdefghijklmnopqrstuvwxyz012345",
        "eyJabcdefghijk.eyJabcdefghijkl.mnopqrstuvwxyz0123",
    ]
    for value in positiveCases {
        if detectorMatches([value], detectors: detectors) == nil {
            throw scannerError(3, "visual secret detector self-test failed closed")
        }
    }
    let splitCredential = try detectionVariants([
        ["Authorization:"],
        ["Bearer"],
        ["abcdefghijkl"],
        ["mnopqrstuvwxyz012345"],
    ])
    guard detectorMatches(splitCredential, detectors: detectors) != nil else {
        throw scannerError(4, "split visual secret detector self-test failed closed")
    }
    let negativeCases = [
        "Connect your OpenAI API key",
        "API key: ••••••••••••••••",
        "No credentials are shown on this screen.",
        "Bearer authentication is available after sign-in.",
    ]
    for value in negativeCases {
        if detectorMatches([value], detectors: detectors) != nil {
            throw scannerError(5, "visual secret detector self-test produced a false positive")
        }
    }
}

private func readRegularData(at path: String, maximum: Int, label: String) throws -> Data {
    let descriptor = Darwin.open(path, O_RDONLY | O_CLOEXEC | O_NOFOLLOW)
    guard descriptor >= 0 else {
        throw scannerError(6, "\(label) is not an accessible no-follow file")
    }
    defer { Darwin.close(descriptor) }

    var metadata = stat()
    guard fstat(descriptor, &metadata) == 0,
          metadata.st_mode & mode_t(S_IFMT) == mode_t(S_IFREG),
          metadata.st_size > 0,
          metadata.st_size <= maximum
    else {
        throw scannerError(7, "\(label) is not one bounded regular file")
    }
    let expectedSize = Int(metadata.st_size)
    var data = Data()
    data.reserveCapacity(expectedSize)
    var buffer = [UInt8](repeating: 0, count: 1024 * 1024)
    while true {
        let count = buffer.withUnsafeMutableBytes { bytes in
            Darwin.read(descriptor, bytes.baseAddress, bytes.count)
        }
        if count < 0 {
            if errno == EINTR { continue }
            throw scannerError(8, "\(label) could not be read")
        }
        if count == 0 { break }
        guard data.count + count <= maximum else {
            throw scannerError(9, "\(label) exceeded its fixed byte limit")
        }
        data.append(contentsOf: buffer[0..<count])
    }
    guard data.count == expectedSize else {
        throw scannerError(10, "\(label) changed while it was read")
    }
    return data
}

private func lowerSHA256(_ data: Data) -> String {
    SHA256.hash(data: data).map { String(format: "%02x", $0) }.joined()
}

private func isLowerSHA256(_ value: String) -> Bool {
    value.count == 64 && value.allSatisfy { character in
        character.isNumber || ("a"..."f").contains(String(character))
    }
}

private func integer(_ value: Any?) -> Int? {
    guard let number = value as? NSNumber,
          CFGetTypeID(number) != CFBooleanGetTypeID()
    else { return nil }
    let result = number.int64Value
    guard result >= 0, result <= Int64(Int.max) else { return nil }
    return Int(result)
}

private func loadSeal(at path: String) throws -> [String: SealEntry] {
    let data = try readRegularData(at: path, maximum: maximumSealBytes, label: "evidence seal")
    guard let root = try JSONSerialization.jsonObject(with: data) as? [String: Any],
          root["schema"] as? String == "goblins-os-hardware-evidence-bundle-v5",
          let framebuffer = root["framebuffer"] as? [String: Any],
          integer(framebuffer["width"]) == canonicalWidth,
          integer(framebuffer["height"]) == canonicalHeight,
          integer(framebuffer["required_png_count"]) == expectedScreenshots.count,
          let entries = root["entries"] as? [[String: Any]]
    else {
        throw scannerError(11, "evidence seal has no exact canonical framebuffer contract")
    }

    var screenshots: [String: SealEntry] = [:]
    for entry in entries where entry["kind"] as? String == "png" {
        guard let name = entry["path"] as? String,
              expectedScreenshots.contains(name),
              screenshots[name] == nil,
              let sha256 = entry["sha256"] as? String,
              isLowerSHA256(sha256),
              let size = integer(entry["size"]),
              size > 0,
              size <= maximumScreenshotBytes,
              let width = integer(entry["width"]),
              let height = integer(entry["height"]),
              width == canonicalWidth,
              height == canonicalHeight
        else {
            throw scannerError(12, "evidence seal has a malformed screenshot entry")
        }
        screenshots[name] = SealEntry(sha256: sha256, size: size, width: width, height: height)
    }
    guard Set(screenshots.keys) == Set(expectedScreenshots) else {
        throw scannerError(13, "evidence seal does not bind the exact 42 screenshots")
    }
    return screenshots
}

private func regularImage(data: Data, expected: SealEntry) throws -> CGImage {
    guard data.count == expected.size,
          lowerSHA256(data) == expected.sha256,
          let source = CGImageSourceCreateWithData(data as CFData, nil),
          CGImageSourceGetCount(source) == 1,
          let imageType = CGImageSourceGetType(source),
          imageType as String == "public.png",
          let image = CGImageSourceCreateImageAtIndex(source, 0, nil),
          image.width == expected.width,
          image.height == expected.height
    else {
        throw scannerError(14, "screenshot bytes do not match the signed canonical PNG entry")
    }
    return image
}

private func tileOrigins(length: Int, tile: Int, step: Int) -> [Int] {
    if length <= tile { return [0] }
    var values = [0]
    while let current = values.last, current + tile < length {
        let next = min(current + step, length - tile)
        if next == current { break }
        values.append(next)
    }
    return values
}

private func recognitionImages(from image: CGImage) throws -> [CGImage] {
    let tileWidth = 2560
    let tileHeight = 1440
    var images: [CGImage] = []
    for y in tileOrigins(length: image.height, tile: tileHeight, step: 1152) {
        for x in tileOrigins(length: image.width, tile: tileWidth, step: 2048) {
            guard let tile = image.cropping(to: CGRect(x: x, y: y, width: tileWidth, height: tileHeight)) else {
                throw scannerError(15, "could not create a bounded OCR tile")
            }
            images.append(tile)
        }
    }
    guard let context = CGContext(
        data: nil,
        width: tileWidth,
        height: tileHeight,
        bitsPerComponent: 8,
        bytesPerRow: 0,
        space: CGColorSpaceCreateDeviceRGB(),
        bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue
    ) else {
        throw scannerError(16, "could not create the whole-frame OCR context")
    }
    context.interpolationQuality = .high
    context.draw(image, in: CGRect(x: 0, y: 0, width: tileWidth, height: tileHeight))
    guard let downsampled = context.makeImage() else {
        throw scannerError(17, "could not create the whole-frame OCR image")
    }
    images.append(downsampled)
    return images
}

private func recognizeText(in image: CGImage) throws -> [[String]] {
    let request = VNRecognizeTextRequest()
    request.revision = VNRecognizeTextRequestRevision3
    request.recognitionLevel = .accurate
    request.usesLanguageCorrection = false
    request.recognitionLanguages = ["en-US", "de-DE"]
    request.minimumTextHeight = 0.003
    let handler = VNImageRequestHandler(cgImage: image, orientation: .up, options: [:])
    try handler.perform([request])
    return (request.results ?? []).map { observation in
        observation.topCandidates(3).map(\.string)
    }
}

do {
    let detectors = try makeDetectors()
    try runDetectorSelfTest(detectors)
    let arguments = Array(CommandLine.arguments.dropFirst())
    guard arguments.count == expectedScreenshots.count + 2,
          arguments.first == "--seal"
    else {
        throw scannerError(18, "visual secret scan requires --seal and exactly 42 screenshots")
    }
    let seal = try loadSeal(at: arguments[1])
    let paths = Array(arguments.dropFirst(2))
    let names = paths.map { URL(fileURLWithPath: $0).lastPathComponent }
    guard names == expectedScreenshots, Set(names).count == expectedScreenshots.count else {
        throw scannerError(19, "visual secret scan screenshot inventory is not exact")
    }

    var observationCount = 0
    for (name, path) in zip(names, paths) {
        guard let expected = seal[name] else {
            throw scannerError(20, "evidence seal omitted a required screenshot")
        }
        let data = try readRegularData(at: path, maximum: maximumScreenshotBytes, label: name)
        let image = try regularImage(data: data, expected: expected)
        var candidateGroups: [[String]] = []
        for recognitionImage in try recognitionImages(from: image) {
            let recognized = try autoreleasepool {
                try recognizeText(in: recognitionImage)
            }
            observationCount += recognized.count
            candidateGroups.append(contentsOf: recognized)
        }
        guard !candidateGroups.isEmpty else {
            throw scannerError(21, "Apple Vision recognized no text in \(name)")
        }
        let variants = try detectionVariants(candidateGroups)
        if let label = detectorMatches(variants, detectors: detectors) {
            fputs("visual-secret-scan: blocked \(name): \(label)\n", stderr)
            exit(1)
        }
    }
    print("visual-secret-scan: pass files=42 observations=\(observationCount) engine=AppleVision revision=3")
} catch {
    fputs("visual-secret-scan: \(error.localizedDescription)\n", stderr)
    exit(1)
}
