// Exit 0 when the console session is unlocked and the main display awake (windows can be visible);
// exit 4 otherwise. Read-only. Usage: swift screen_state.swift
import CoreGraphics
let d = CGSessionCopyCurrentDictionary() as? [String: Any] ?? [:]
let locked = (d["CGSSessionScreenIsLocked"] as? Bool) ?? ((d["CGSSessionScreenIsLocked"] as? Int) == 1)
let asleep = CGDisplayIsAsleep(CGMainDisplayID()) != 0
print("screen locked=\(locked) displayAsleep=\(asleep)")
exit(locked || asleep ? 4 : 0)
