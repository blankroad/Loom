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
