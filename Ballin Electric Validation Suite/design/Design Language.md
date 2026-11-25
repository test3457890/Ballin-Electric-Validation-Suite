# Ballin Electric design language

Version 0.2 | Initial visual direction | Rust / FLTK-rs

## Design intent

Mature professional manufacturing and diagnostics software, with the clarity and density of late-1990s through 2010s desktop utilities. It should feel like a tool used daily by repair technicians, research engineers and factory quality-control operators. Timeless rather than deliberately obsolete.

## References

HWiNFO (dense sensor tables), CPU-Z (traditional tabs), Windows Computer Management and Event Viewer (tree and log), Prime95 (utilitarian test controls), PassMark BurnInTest and PC-Doctor Factory (manufacturing test flows), and automotive diagnostic software such as Toyota Techstream (fault codes and troubleshooting). These are behavioral and visual inspirations only. Do not copy proprietary icons, graphics or layouts verbatim.

## Layout and interaction

| Region | Specification |
| --- | --- |
| Title bar | Small original steel-sphere mark, product name, normal window controls |
| Menu | File, System, Diagnostics, Tests, Manufacturing, Reports, Tools, View, Help |
| Toolbar | Scan, Run Tests, Stop, Results, Save, Export, Print; compact labeled buttons |
| Left pane | Expandable hardware tree, device groups, manufacturing and service records |
| Main pane | Context heading, device ID/profile, conventional tabs and sortable tables |
| Details | Selected result, measured values, diagnostic code, evidence and follow-up action |
| Event log | Resizable, timestamped, filterable text output with selectable/copyable lines |
| Status bar | Connection mode, operator, privileges, error count and app version |

Use keyboard shortcuts, visible focus, context menus, resizable columns and split panes. Keep the stop control visible and responsive during any test. Disabled controls must have a reason. Support keyboard-only use, adjustable font size, high DPI, high contrast and a practical minimum window around 1024 x 768. Validate accessibility on Windows and Linux; FLTK is not a guarantee of OS-native accessibility.

## Visual tokens

The machine-readable palette is in [Design Tokens.json](Design%20Tokens.json). The default Industrial theme uses cool gray chrome, white data panes, dark slate accent and subtle borders. Classic uses warm gray and navy; Dark uses charcoal and readable status colors. Theme alternatives must preserve information architecture and affordances.

Use Segoe UI or Tahoma, platform-appropriate fallbacks, and Consolas or another monospaced font for logs. Aim for 9 to 10 pt default desktop text with user-adjustable scaling, 16 to 24 logical pixel toolbar icons, square corners, 1 px separators and tight spacing. Do not hard-code pixels in ways that prevent accessibility scaling.

Status labels: PASS, FAIL, WARNING, RUNNING, PENDING, NOT RUN, UNSUPPORTED, ABORTED. Always show a readable text label, not color alone. Unknown readings remain unknown; never render unavailable readings as zero.

## Main window and views

Use the [static PNG](assets/Main%20Window%20Reference.png) or [editable SVG](assets/Main%20Window%20Reference.svg) as the initial layout reference. It illustrates a failed GPU memory test and a shipment HOLD. These are fictional design examples, not actual readings. The real UI must derive everything from the engine.

- Inventory: device properties, expected versus detected hardware, firmware and IDs.
- Sensors: dense sortable table (current, min, max, average, units, freshness) with optional instrument-style line chart.
- Tests: profile selection, test coverage, elapsed time, results, cancellation and evidence.
- Errors: chronological codes, confirmed observations, suspected causes and next safe steps.
- Manufacturing: serial/BOM records, manual checks, linked rework and explicit release review.
- Reports and service: printable factory report, historical runs and baseline comparison.

Graphs must include axes, units, legends and selectable sensors. Use understated flat traces, no decorative chart effects.

## Branding and prohibitions

Use Ballin Electric System Validation Suite (short name BE Validation). Subtitle: Hardware Diagnostics & Manufacturing Quality Assurance. The original sphere is a small steel-bearing-inspired mark, not a third-party logo. The SVG mockup includes an illustrative mark; trademark clearance is not complete.

No SaaS dashboard cards, oversized metrics, pill buttons, glass effects, colorful gradient backgrounds, floating navigation, animations, gaming RGB, webview-based layout or chatbot-first experience. No misleading indicators, decorative-only actions or fabricated test results. A modest metallic logo is the sole exception to the flat UI material rule.

## Implementation notes

Use FLTK MenuBar, Tree, Tabs, Table or TableRow, Tile, Progress, TextDisplay, and conventional dialogs. Keep the view layer separate from the core. Validate the real rendered app at typical desktop sizes and multiple DPI settings. The static reference is a design aid, not application source or a requirement to draw custom fake controls.
