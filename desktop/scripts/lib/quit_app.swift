// Ask ONE app (by pid) to quit through AppKit: NSRunningApplication.terminate() sends the standard
// quit Apple Event (same applicationShouldTerminate path as Cmd-Q / menu Quit). Not UI input, no
// Accessibility/System Events. Usage: swift quit_app.swift <pid>
import AppKit
guard let pid = Int32(CommandLine.arguments.dropFirst().first ?? ""),
      let app = NSRunningApplication(processIdentifier: pid) else { print("quit_app: no such app pid"); exit(2) }
print("quit_app: pid \(pid) bundle \(app.bundleIdentifier ?? "nil") terminate() -> \(app.terminate())")
