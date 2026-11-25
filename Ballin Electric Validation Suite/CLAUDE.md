# Claude Code project instructions

Read `docs/Development Specification.md` in full before implementation. It is the authoritative functional and technical brief. `design/Design Language.md`, `design/Design Tokens.json`, and `design/assets/Main Window Reference.png` define the initial visual target. The reference uses simulated data only.

## Mission

Build a real professional OEM validation platform in Rust. Reliability, diagnostics and manufacturing release control take precedence over benchmarking. Use FLTK-rs for the initial classic desktop interface, Cargo, SQLite and modular platform/worker interfaces. Avoid C++ for first-party application code, except narrowly documented native SDK/kernel interfaces when required. Do not use Electron, React, Tailwind or a webview shell.

## Execution

- Inspect the repository and preserve existing working code and user changes.
- Create an implementation plan, architecture record and capability matrix. Follow Milestones 0 through 6 in the full specification; do not stop at planning or a GUI shell.
- Implement actual hardware inventory and at least one safe known-answer diagnostic before advanced workloads. Use versioned profiles, typed results, structured error codes and clear test coverage.
- Use isolated workers, explicit cancellation, watchdog design, bounded resources and a conservative safety policy. No intensive hardware tests without explicit authorization and an approved safe hardware profile.
- Make every UI control useful or visibly unavailable with a reason. Never invent hardware readings, diagnostic results or progress. Keep the production path separate from mock fixtures.
- Verify licensing and supported hardware APIs before bundling third-party software or enabling a vendor adapter. Record all decisions and blockers.
- Run safe formatting, lint, build and automated fixture tests. Document the exact commands and actual results. Do not automatically run physical stress tests in CI.
- Update `docs/Development Status.md` and dependency notices at each milestone. Clearly distinguish planned, implemented, fixture-tested and physically verified features. Do not push, publish or configure external services without authorization.

## Design guardrails

Use compact menu bars, rectangular controls, resizable split panes, technical tables, visible status labels, a selectable event log and a reachable Stop action. Preserve the Industrial visual identity. Classic and Dark are optional later themes. Do not introduce cards, pill buttons, large decorative typography or chatbot-first navigation. If FLTK-rs cannot meet accessibility needs, document alternatives and request approval before changing toolkit.

## Acceptance

Never report unsupported tests as passing. A required failed, missing, aborted or unsupported test must block shipment approval. The first milestone is a buildable real Rust application shell; the next is useful real hardware diagnostics. Follow the full specification for all subsequent acceptance criteria.
