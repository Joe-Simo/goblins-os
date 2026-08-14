import CryptoKit
import Foundation
import Security

private let authoritySchema = "goblins-os-display-authority2-keychain-policy-v1"
private let maximumLockInterval: UInt32 = 300
private let requiredPromptBit: UInt16 = 0x0001

private enum AuthorityToolError: Error, CustomStringConvertible {
  case failure(String)

  var description: String {
    switch self {
    case .failure(let message):
      return message
    }
  }
}

private struct ACLPolicyEntry {
  let authorizations: Set<String>
  let applicationListPresent: Bool
  let trustedApplicationCount: Int
  let promptSelector: UInt16

  var normalized: [String: Any] {
    [
      "application_list_present": applicationListPresent,
      "authorizations": authorizations.sorted(),
      "prompt_selector": Int(promptSelector),
      "trusted_application_count": trustedApplicationCount,
    ]
  }
}

private struct IdentityContext {
  let keychain: SecKeychain
  let identity: SecIdentity
  let privateKey: SecKey
  let certificateFingerprint: String
  let identityCount: Int
  let privateKeyCount: Int
}

private func fail(_ message: String) throws -> Never {
  throw AuthorityToolError.failure(message)
}

private func checkStatus(_ status: OSStatus, _ operation: String) throws {
  guard status == errSecSuccess else {
    let detail = SecCopyErrorMessageString(status, nil) as String? ?? "OSStatus \(status)"
    try fail("\(operation) failed: \(detail) (\(status))")
  }
}

private func sha256Hex(_ data: Data) -> String {
  SHA256.hash(data: data).map { String(format: "%02x", $0) }.joined()
}

private func jsonData(_ value: Any) throws -> Data {
  try JSONSerialization.data(withJSONObject: value, options: [.sortedKeys])
}

private func printJSON(_ value: Any) throws {
  FileHandle.standardOutput.write(try jsonData(value))
  FileHandle.standardOutput.write(Data([0x0a]))
}

private func openKeychain(_ path: String) throws -> SecKeychain {
  var keychain: SecKeychain?
  let status = path.withCString { SecKeychainOpen($0, &keychain) }
  try checkStatus(status, "open dedicated Keychain")
  guard let keychain else {
    try fail("open dedicated Keychain returned no Keychain reference")
  }
  return keychain
}

private func keychainPath(_ keychain: SecKeychain) throws -> String {
  var capacity: UInt32 = 4096
  var buffer = [CChar](repeating: 0, count: Int(capacity) + 1)
  try checkStatus(
    SecKeychainGetPath(keychain, &capacity, &buffer),
    "read dedicated Keychain path"
  )
  return String(cString: buffer)
}

private func cfArrayObjects<T>(_ array: CFArray, as _: T.Type) -> [T] {
  (0..<CFArrayGetCount(array)).map { index in
    let pointer = CFArrayGetValueAtIndex(array, index)
    return unsafeBitCast(pointer, to: T.self)
  }
}

private func queryArray(_ query: CFDictionary, _ operation: String) throws -> CFArray {
  var result: CFTypeRef?
  let status = SecItemCopyMatching(query, &result)
  if status == errSecItemNotFound {
    return [] as CFArray
  }
  try checkStatus(status, operation)
  guard let result, CFGetTypeID(result) == CFArrayGetTypeID() else {
    try fail("\(operation) returned a non-array result")
  }
  return unsafeBitCast(result, to: CFArray.self)
}

private func copyCommonName(_ certificate: SecCertificate) throws -> String {
  var commonName: CFString?
  try checkStatus(
    SecCertificateCopyCommonName(certificate, &commonName),
    "read authority certificate common name"
  )
  guard let commonName else {
    try fail("authority certificate has no common name")
  }
  return commonName as String
}

private func resolveIdentity(
  keychain: SecKeychain,
  identityName: String,
  expectedFingerprint: String
) throws -> IdentityContext {
  let identityQuery: [CFString: Any] = [
    kSecClass: kSecClassIdentity,
    kSecMatchLimit: kSecMatchLimitAll,
    kSecMatchSearchList: [keychain] as CFArray,
    kSecReturnRef: true,
  ]
  let identityArray = try queryArray(
    identityQuery as CFDictionary,
    "find identities in dedicated Keychain"
  )
  let identities = cfArrayObjects(identityArray, as: SecIdentity.self)

  var matches: [(SecIdentity, SecKey, String)] = []
  for identity in identities {
    var certificate: SecCertificate?
    try checkStatus(
      SecIdentityCopyCertificate(identity, &certificate),
      "read authority identity certificate"
    )
    guard let certificate else {
      try fail("authority identity returned no certificate")
    }
    let commonName = try copyCommonName(certificate)
    let fingerprint = sha256Hex(SecCertificateCopyData(certificate) as Data)
    guard commonName == identityName, fingerprint == expectedFingerprint else {
      continue
    }
    var privateKey: SecKey?
    try checkStatus(
      SecIdentityCopyPrivateKey(identity, &privateKey),
      "read authority identity private key"
    )
    guard let privateKey else {
      try fail("authority identity returned no private key")
    }
    matches.append((identity, privateKey, fingerprint))
  }

  guard matches.count == 1 else {
    try fail(
      "dedicated Keychain must contain exactly one identity matching the pinned Authority 2 certificate; found \(matches.count)"
    )
  }

  let privateKeyQuery: [CFString: Any] = [
    kSecAttrKeyClass: kSecAttrKeyClassPrivate,
    kSecClass: kSecClassKey,
    kSecMatchLimit: kSecMatchLimitAll,
    kSecMatchSearchList: [keychain] as CFArray,
    kSecReturnRef: true,
  ]
  let privateKeyArray = try queryArray(
    privateKeyQuery as CFDictionary,
    "find private keys in dedicated Keychain"
  )
  let privateKeys = cfArrayObjects(privateKeyArray, as: SecKey.self)
  guard privateKeys.count == 1 else {
    try fail(
      "dedicated Keychain must contain exactly one private key; found \(privateKeys.count)"
    )
  }
  guard CFEqual(privateKeys[0], matches[0].1) else {
    try fail("the dedicated Keychain private key does not match the pinned Authority 2 identity")
  }

  return IdentityContext(
    keychain: keychain,
    identity: matches[0].0,
    privateKey: matches[0].1,
    certificateFingerprint: matches[0].2,
    identityCount: matches.count,
    privateKeyCount: privateKeys.count
  )
}

private func copyACLs(_ privateKey: SecKey) throws -> (SecAccess, [SecACL]) {
  var access: SecAccess?
  let keychainItem = unsafeBitCast(privateKey, to: SecKeychainItem.self)
  try checkStatus(
    SecKeychainItemCopyAccess(keychainItem, &access),
    "copy authority private-key access policy"
  )
  guard let access else {
    try fail("authority private key returned no access policy")
  }
  var aclArray: CFArray?
  try checkStatus(
    SecAccessCopyACLList(access, &aclArray),
    "copy authority private-key ACLs"
  )
  guard let aclArray else {
    try fail("authority private key returned no ACL list")
  }
  return (access, cfArrayObjects(aclArray, as: SecACL.self))
}

private func readPolicyEntry(_ acl: SecACL) throws -> ACLPolicyEntry {
  let authorizationArray = SecACLCopyAuthorizations(acl)
  let authorizations = Set(
    cfArrayObjects(authorizationArray, as: CFString.self).map { $0 as String }
  )
  guard !authorizations.isEmpty else {
    try fail("authority private key contains an ACL with no authorizations")
  }

  var applicationList: CFArray?
  var descriptor: CFString?
  var promptSelector = SecKeychainPromptSelector(rawValue: 0)
  try checkStatus(
    SecACLCopyContents(acl, &applicationList, &descriptor, &promptSelector),
    "read authority private-key ACL contents"
  )
  let trustedApplicationCount = applicationList.map(CFArrayGetCount) ?? -1
  return ACLPolicyEntry(
    authorizations: authorizations,
    applicationListPresent: applicationList != nil,
    trustedApplicationCount: trustedApplicationCount,
    promptSelector: promptSelector.rawValue
  )
}

private let authorizationSign = kSecACLAuthorizationSign as String
private let authorizationChangeACL = kSecACLAuthorizationChangeACL as String
private let authorizationEncrypt = kSecACLAuthorizationEncrypt as String
private let authorizationIntegrity = kSecACLAuthorizationIntegrity as String
private let authorizationAny = kSecACLAuthorizationAny as String
private let authorizationPartition = kSecACLAuthorizationPartitionID as String

private let allowedAuthorizations: Set<String> = [
  authorizationSign,
  authorizationChangeACL,
  authorizationEncrypt,
  authorizationIntegrity,
]

private func validateIdentityCounts(identityCount: Int, privateKeyCount: Int) throws {
  guard identityCount == 1 else {
    try fail("Authority 2 policy requires exactly one matching identity")
  }
  guard privateKeyCount == 1 else {
    try fail("Authority 2 policy requires exactly one private key")
  }
}

private func validatePolicy(_ entries: [ACLPolicyEntry]) throws -> [String: Any] {
  let unexpected = Set(entries.flatMap(\.authorizations)).subtracting(allowedAuthorizations)
  guard unexpected.isEmpty else {
    try fail(
      "authority private key contains prohibited authorizations: \(unexpected.sorted().joined(separator: ", "))"
    )
  }
  guard !entries.contains(where: { $0.authorizations.contains(authorizationAny) }) else {
    try fail("authority private key contains a broad any authorization")
  }
  guard !entries.contains(where: { $0.authorizations.contains(authorizationPartition) }) else {
    try fail("authority private key contains a partition authorization")
  }

  let signContainingEntries = entries.filter {
    $0.authorizations.contains(authorizationSign)
  }
  guard signContainingEntries.count == 1,
    signContainingEntries[0].authorizations == [authorizationSign]
  else {
    try fail("authority private key must contain exactly one sign-only ACL")
  }
  let changeContainingEntries = entries.filter {
    $0.authorizations.contains(authorizationChangeACL)
  }
  guard changeContainingEntries.count == 1,
    changeContainingEntries[0].authorizations == [authorizationChangeACL]
  else {
    try fail("authority private key must contain exactly one change_acl-only ACL")
  }

  // A classic macOS key access object retains one safe encrypt ACL and one
  // integrity ACL. Encrypt is the non-secret public-key operation; integrity
  // protects the access object itself. They are the only non-authority ACLs
  // retained by Authority 2, and each must be isolated from restricted use.
  let encryptEntries = entries.filter { $0.authorizations == [authorizationEncrypt] }
  let integrityEntries = entries.filter { $0.authorizations == [authorizationIntegrity] }
  guard encryptEntries.count == 1, integrityEntries.count == 1, entries.count == 4 else {
    try fail(
      "authority private key must contain only sign, change_acl, encrypt, and integrity ACLs"
    )
  }

  let signEntry = signContainingEntries[0]
  guard signEntry.applicationListPresent, signEntry.trustedApplicationCount == 0 else {
    try fail("authority sign ACL must contain an explicit empty trusted-application list")
  }
  guard signEntry.promptSelector & requiredPromptBit == requiredPromptBit else {
    try fail("authority sign ACL must require the Keychain passphrase")
  }

  let changeEntry = changeContainingEntries[0]
  guard changeEntry.applicationListPresent, changeEntry.trustedApplicationCount == 0 else {
    try fail("authority change_acl ACL must contain an explicit empty trusted-application list")
  }
  guard changeEntry.promptSelector & requiredPromptBit == requiredPromptBit else {
    try fail("authority change_acl ACL must require the Keychain passphrase")
  }

  let normalizedEntries = entries.map(\.normalized).sorted {
    let left = (try? jsonData($0)) ?? Data()
    let right = (try? jsonData($1)) ?? Data()
    return left.lexicographicallyPrecedes(right)
  }
  let policyDigest = sha256Hex(try jsonData(normalizedEntries))
  return [
    "change_acl_prompt_selector": Int(changeEntry.promptSelector),
    "change_acl_trusted_application_count": changeEntry.trustedApplicationCount,
    "policy_sha256": policyDigest,
    "sign_prompt_selector": Int(signEntry.promptSelector),
    "sign_trusted_application_count": signEntry.trustedApplicationCount,
  ]
}

private func keychainContainerReport(_ keychain: SecKeychain) throws -> [String: Any] {
  var status: SecKeychainStatus = 0
  try checkStatus(SecKeychainGetStatus(keychain, &status), "read dedicated Keychain status")

  var settings = SecKeychainSettings(
    version: UInt32(SEC_KEYCHAIN_SETTINGS_VERS1),
    lockOnSleep: DarwinBoolean(false),
    useLockInterval: DarwinBoolean(false),
    lockInterval: 0
  )
  try checkStatus(
    SecKeychainCopySettings(keychain, &settings),
    "read dedicated Keychain settings"
  )
  guard settings.lockOnSleep.boolValue else {
    try fail("dedicated Keychain must lock when the system sleeps")
  }
  guard settings.useLockInterval.boolValue,
    settings.lockInterval > 0,
    settings.lockInterval <= maximumLockInterval
  else {
    try fail("dedicated Keychain must use a lock interval between 1 and 300 seconds")
  }

  var searchList: CFArray?
  try checkStatus(
    SecKeychainCopyDomainSearchList(.user, &searchList),
    "read user Keychain search list"
  )
  let inSearchList =
    searchList.map {
      cfArrayObjects($0, as: SecKeychain.self).contains { CFEqual($0, keychain) }
    } ?? false
  guard !inSearchList else {
    try fail("dedicated Keychain must not appear in the user search list")
  }

  var defaultKeychain: SecKeychain?
  try checkStatus(
    SecKeychainCopyDomainDefault(.user, &defaultKeychain),
    "read user default Keychain"
  )
  let isDefault = defaultKeychain.map { CFEqual($0, keychain) } ?? false
  guard !isDefault else {
    try fail("dedicated Keychain must not be the user default Keychain")
  }

  return [
    "in_user_search_list": inSearchList,
    "is_user_default": isDefault,
    "keychain": try keychainPath(keychain),
    "lock_interval_seconds": Int(settings.lockInterval),
    "lock_on_sleep": settings.lockOnSleep.boolValue,
    "unlocked": status & SecKeychainStatus(kSecUnlockStateStatus) != 0,
  ]
}

private func audit(
  keychainPath: String,
  identityName: String,
  expectedFingerprint: String
) throws -> [String: Any] {
  let keychain = try openKeychain(keychainPath)
  var report = try keychainContainerReport(keychain)
  guard report["unlocked"] as? Bool == true else {
    try fail("dedicated Keychain must be unlocked only through SecurityAgent before ACL audit")
  }
  let context = try resolveIdentity(
    keychain: keychain,
    identityName: identityName,
    expectedFingerprint: expectedFingerprint
  )
  try validateIdentityCounts(
    identityCount: context.identityCount,
    privateKeyCount: context.privateKeyCount
  )
  let (_, acls) = try copyACLs(context.privateKey)
  let policy = try validatePolicy(acls.map(readPolicyEntry))
  report.merge(policy) { _, new in new }
  report.merge([
    "certificate_sha256": context.certificateFingerprint,
    "identity": identityName,
    "identity_count": context.identityCount,
    "private_key_count": context.privateKeyCount,
    "schema": authoritySchema,
  ]) { _, new in new }
  return report
}

private func matchingACLs(_ access: SecAccess, _ authorization: CFString) -> [SecACL] {
  guard let array = SecAccessCopyMatchingACLList(access, authorization) else {
    return []
  }
  return cfArrayObjects(array, as: SecACL.self)
}

private func harden(
  keychainPath: String,
  identityName: String,
  expectedFingerprint: String
) throws -> [String: Any] {
  let keychain = try openKeychain(keychainPath)
  let container = try keychainContainerReport(keychain)
  guard container["unlocked"] as? Bool == true else {
    try fail("dedicated Keychain must be unlocked through SecurityAgent before hardening")
  }
  let context = try resolveIdentity(
    keychain: keychain,
    identityName: identityName,
    expectedFingerprint: expectedFingerprint
  )
  let (access, originalACLs) = try copyACLs(context.privateKey)
  let signACLs = matchingACLs(access, kSecACLAuthorizationSign)
  let changeACLs = matchingACLs(access, kSecACLAuthorizationChangeACL)
  guard signACLs.count == 1 else {
    try fail("cannot safely harden an authority key without exactly one sign-matching ACL")
  }
  guard changeACLs.count == 1 else {
    try fail("cannot safely harden an authority key without exactly one change_acl-matching ACL")
  }
  guard !CFEqual(signACLs[0], changeACLs[0]) else {
    try fail("cannot safely harden a single ACL that controls both signing and ACL changes")
  }

  let emptyApplications = [] as CFArray
  let prompt = SecKeychainPromptSelector(rawValue: requiredPromptBit)
  try checkStatus(
    SecACLSetContents(
      signACLs[0],
      emptyApplications,
      identityName as CFString,
      prompt
    ),
    "set Authority 2 sign prompt policy"
  )
  try checkStatus(
    SecACLUpdateAuthorizations(
      signACLs[0],
      [kSecACLAuthorizationSign] as CFArray
    ),
    "restrict Authority 2 key to signing"
  )
  try checkStatus(
    SecACLSetContents(
      changeACLs[0],
      emptyApplications,
      identityName as CFString,
      prompt
    ),
    "set Authority 2 ACL-change prompt policy"
  )
  try checkStatus(
    SecACLUpdateAuthorizations(
      changeACLs[0],
      [kSecACLAuthorizationChangeACL] as CFArray
    ),
    "restrict Authority 2 ACL ownership"
  )

  for acl in originalACLs where !CFEqual(acl, signACLs[0]) && !CFEqual(acl, changeACLs[0]) {
    let entry = try readPolicyEntry(acl)
    if entry.authorizations != [authorizationEncrypt]
      && entry.authorizations != [authorizationIntegrity]
    {
      try checkStatus(SecACLRemove(acl), "remove prohibited Authority 2 ACL")
    }
  }
  try checkStatus(
    SecKeychainItemSetAccess(
      unsafeBitCast(context.privateKey, to: SecKeychainItem.self),
      access
    ),
    "persist Authority 2 private-key access policy"
  )
  return try audit(
    keychainPath: keychainPath,
    identityName: identityName,
    expectedFingerprint: expectedFingerprint
  )
}

private func expectRejected(_ name: String, _ operation: () throws -> Void) throws {
  do {
    try operation()
  } catch {
    return
  }
  try fail("self-test accepted unsafe policy: \(name)")
}

private func runSelfTest() throws {
  let sign = ACLPolicyEntry(
    authorizations: [authorizationSign],
    applicationListPresent: true,
    trustedApplicationCount: 0,
    promptSelector: requiredPromptBit
  )
  let change = ACLPolicyEntry(
    authorizations: [authorizationChangeACL],
    applicationListPresent: true,
    trustedApplicationCount: 0,
    promptSelector: requiredPromptBit
  )
  let encrypt = ACLPolicyEntry(
    authorizations: [authorizationEncrypt],
    applicationListPresent: false,
    trustedApplicationCount: -1,
    promptSelector: 0
  )
  let integrity = ACLPolicyEntry(
    authorizations: [authorizationIntegrity],
    applicationListPresent: false,
    trustedApplicationCount: -1,
    promptSelector: 0
  )
  _ = try validatePolicy([sign, change, encrypt, integrity])
  try validateIdentityCounts(identityCount: 1, privateKeyCount: 1)

  try expectRejected("broad any") {
    _ = try validatePolicy([
      ACLPolicyEntry(
        authorizations: [authorizationAny],
        applicationListPresent: false,
        trustedApplicationCount: -1,
        promptSelector: 0
      ),
      change,
    ])
  }
  try expectRejected("trusted /usr/bin/security") {
    _ = try validatePolicy([
      ACLPolicyEntry(
        authorizations: [authorizationSign],
        applicationListPresent: true,
        trustedApplicationCount: 1,
        promptSelector: requiredPromptBit
      ),
      change,
    ])
  }
  try expectRejected("sign prompt selector zero") {
    _ = try validatePolicy([
      ACLPolicyEntry(
        authorizations: [authorizationSign],
        applicationListPresent: true,
        trustedApplicationCount: 0,
        promptSelector: 0
      ),
      change,
    ])
  }
  try expectRejected("sign plus export_clear") {
    _ = try validatePolicy([
      ACLPolicyEntry(
        authorizations: [authorizationSign, kSecACLAuthorizationExportClear as String],
        applicationListPresent: true,
        trustedApplicationCount: 0,
        promptSelector: requiredPromptBit
      ),
      change,
    ])
  }
  try expectRejected("separate decrypt authorization") {
    _ = try validatePolicy([
      sign,
      change,
      ACLPolicyEntry(
        authorizations: [kSecACLAuthorizationDecrypt as String],
        applicationListPresent: true,
        trustedApplicationCount: 0,
        promptSelector: requiredPromptBit
      ),
    ])
  }
  try expectRejected("partition authorization") {
    _ = try validatePolicy([
      sign,
      change,
      ACLPolicyEntry(
        authorizations: [authorizationPartition],
        applicationListPresent: false,
        trustedApplicationCount: -1,
        promptSelector: 0
      ),
    ])
  }
  try expectRejected("change_acl prompt selector zero") {
    _ = try validatePolicy([
      sign,
      ACLPolicyEntry(
        authorizations: [authorizationChangeACL],
        applicationListPresent: true,
        trustedApplicationCount: 0,
        promptSelector: 0
      ),
    ])
  }
  try expectRejected("duplicate private keys") {
    try validateIdentityCounts(identityCount: 1, privateKeyCount: 2)
  }
  try expectRejected("combined sign and encrypt authorization") {
    _ = try validatePolicy([
      sign,
      change,
      encrypt,
      integrity,
      ACLPolicyEntry(
        authorizations: [authorizationSign, authorizationEncrypt],
        applicationListPresent: true,
        trustedApplicationCount: 0,
        promptSelector: requiredPromptBit
      ),
    ])
  }
  try expectRejected("duplicate sign ACL") {
    _ = try validatePolicy([sign, sign, change, encrypt, integrity])
  }
  try printJSON([
    "negative_cases": 10,
    "schema": authoritySchema,
    "status": "pass",
  ])
}

private func parseOptions(_ arguments: ArraySlice<String>) throws -> [String: String] {
  var options: [String: String] = [:]
  var index = arguments.startIndex
  while index < arguments.endIndex {
    let key = arguments[index]
    guard key.hasPrefix("--") else {
      try fail("unexpected argument: \(key)")
    }
    let valueIndex = arguments.index(after: index)
    guard valueIndex < arguments.endIndex else {
      try fail("missing value for \(key)")
    }
    guard options[key] == nil else {
      try fail("duplicate option: \(key)")
    }
    options[key] = arguments[valueIndex]
    index = arguments.index(after: valueIndex)
  }
  return options
}

private func requireOption(_ options: [String: String], _ name: String) throws -> String {
  guard let value = options[name], !value.isEmpty else {
    try fail("missing required option \(name)")
  }
  return value
}

private func run() throws {
  let arguments = CommandLine.arguments
  guard arguments.count >= 2 else {
    try fail("usage: display-authority2-keychain <status|audit|harden|self-test> [options]")
  }
  let command = arguments[1]
  if command == "self-test" {
    guard arguments.count == 2 else {
      try fail("self-test takes no options")
    }
    try runSelfTest()
    return
  }
  let options = try parseOptions(arguments.dropFirst(2))
  let keychainPath = try requireOption(options, "--keychain")
  let allowed: Set<String>
  if command == "status" {
    allowed = ["--keychain"]
  } else {
    allowed = ["--certificate-sha256", "--identity", "--keychain"]
  }
  let unexpected = Set(options.keys).subtracting(allowed)
  guard unexpected.isEmpty else {
    try fail("unexpected options: \(unexpected.sorted().joined(separator: ", "))")
  }

  switch command {
  case "status":
    try printJSON(keychainContainerReport(try openKeychain(keychainPath)))
  case "audit", "harden":
    let identity = try requireOption(options, "--identity")
    let fingerprint = try requireOption(options, "--certificate-sha256").lowercased()
    guard fingerprint.range(of: "^[0-9a-f]{64}$", options: .regularExpression) != nil else {
      try fail("--certificate-sha256 must be one lowercase SHA-256 value")
    }
    let report =
      if command == "audit" {
        try audit(
          keychainPath: keychainPath,
          identityName: identity,
          expectedFingerprint: fingerprint
        )
      } else {
        try harden(
          keychainPath: keychainPath,
          identityName: identity,
          expectedFingerprint: fingerprint
        )
      }
    try printJSON(report)
  default:
    try fail("unknown command: \(command)")
  }
}

do {
  try run()
} catch {
  FileHandle.standardError.write(Data("display-authority2-keychain: \(error)\n".utf8))
  exit(2)
}
