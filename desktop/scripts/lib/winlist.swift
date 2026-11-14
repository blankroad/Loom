// Read-only native window diagnosis: `swift desktop/scripts/lib/winlist.swift <pid>...`
// Lists CGWindowList entries owned by the pids (all + on-screen) and NSRunningApplication state.
// Needs no Accessibility/Screen Recording permission (window titles are then omitted by macOS).
import CoreGraphics
import AppKit
let pids = CommandLine.arguments.dropFirst().compactMap { Int32($0) }
for opt in [CGWindowListOption.optionAll, .optionOnScreenOnly] {
  let list = CGWindowListCopyWindowInfo(opt, kCGNullWindowID) as? [[String: Any]] ?? []
  for w in list {
    guard let p = w[kCGWindowOwnerPID as String] as? Int32, pids.contains(p) else { continue }
    print(opt == .optionAll ? "ALL" : "ONSCREEN", "pid", p, "win", w[kCGWindowNumber as String] ?? "-", "layer", w[kCGWindowLayer as String] ?? "-",
          "onscreen", w[kCGWindowIsOnscreen as String] ?? "-", "bounds", w[kCGWindowBounds as String] ?? "-", "alpha", w[kCGWindowAlpha as String] ?? "-")
  }
}
for p in pids {
  if let a = NSRunningApplication(processIdentifier: p) {
    print("app pid", p, "bundle", a.bundleIdentifier ?? "nil", "url", a.bundleURL?.path ?? "nil", "policy", a.activationPolicy.rawValue, "active", a.isActive, "hidden", a.isHidden, "finished", a.isFinishedLaunching)
  } else { print("app pid", p, "NOT an NSRunningApplication") }
}
