# Loom UI and viewers

- The user watches in a browser at `http://localhost:<port>`. A viewer connects with a one-time
  login code that only reaches a **human terminal**: `loom ui` in the user's own terminal opens the
  browser. From a non-terminal (you) `loom ui` only prints advice — that is expected.
- `no_viewer_attached` / `no_visible_viewer` ⇒ nobody can see what you do: ask the user to run
  `loom ui` (or bring the Loom tab forward) before risky or important steps.
- `loom ui open <panel> [--filter F] [--at CURSOR]` asks the UI to focus a panel (`ui.focus_request`).
- `loom mark "<label>" [--note …]` puts a marker on the shared timeline; use it before/after key
  steps so the user and later reviews can find them (`--since mark:<label>` works as a cursor).
- Viewer rights: `watch` (see + STOP) · `operate` (type, send, release STOP, recovery) ·
  `approve` (approvals, policy). Agents have none of the viewer rights: you cannot type into the
  user's terminal panel, release STOP or approve.
- The user's typing into a device (e.g. a UART terminal) appears in your next result as
  `others_since_last` with origin `user` and kind `<domain>.input`.

## Isolated simulator UI acceptance

When the user explicitly requests direct browser/UI acceptance and you created the local test
daemon with its own `LOOM_HOME`, a temporary **watch** viewer is a test fixture. You may use the
daemon's normal `/api/v1/bootstrap/new?rights=watch` and bootstrap exchange for that fixture.
Keep the agent token, one-time code and cookie out of evidence/log output. Do not retrieve a
human-issued code, use an existing user's daemon/session, change policy, or issue operate/approve
credentials through this exception. Normal user-device sessions follow the human-terminal rule.

Use the real UI against the same daemon as the skill-based observer. Wait for the observer's
initial snapshot and the UI's connected/visible state before sending a fresh test marker. Compare
the new request ID, epoch/seq and exact bytes in both consumers; a historical query or a static
mock is not simultaneous receive evidence. Record viewer rights and visibility, not credentials.
If navigating an already-loaded SPA to a bootstrap fragment leaves it unconsumed, reload that
page so startup exchanges and removes the fragment. Never treat an unauthenticated retrying UI
as a pass.
