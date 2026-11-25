# Accessibility and display scaling: findings and limitations

The findings below come from the FLTK-rs 1.5.24 build (FLTK 1.4) tested under Xvfb on Linux.

| Requirement | Status | Evidence / notes |
| --- | --- | --- |
| Keyboard operation | Partially verified | All actions are in menus with mnemonics and shortcuts: F5 scan, F9 run, Shift+F5 stop, Ctrl+T targeted test, Ctrl+S/E/P save, export and print, Ctrl+N new unit, Ctrl+= / Ctrl+- / Ctrl+0 zoom, F1 about. Tab moves focus. Arrow keys move the row selection in result tables (verified with xdotool: one row per key press). Tree navigation uses FLTK defaults. |
| Visible focus | Enabled | `app::set_visible_focus(true)` |
| Status not by color alone | Yes | Every status is a text label (PASS, FAIL, …); color is additional. |
| Adjustable text size / high DPI | Implemented | View ▸ Zoom uses FLTK's per-screen scale factor, which scales the whole UI. FLTK 1.4 also reads the OS scale factor. Not yet verified on physical 4K displays or Windows per-monitor DPI. |
| Minimum window size | 1024 × 768 | Enforced with `size_range`. Split panes are resizable (Tile). |
| Screen readers (Narrator, NVDA, Orca) | **Not supported** | FLTK 1.4 has no native accessibility API bridge (no UI Automation, MSAA or AT-SPI). FLTK-drawn widgets are invisible to screen readers. This is a real limitation for customer-facing use. |
| High-contrast OS themes | Not honored automatically | Industrial, Classic and Dark themes are provided. A dedicated high-contrast theme is a candidate. |
| Escape key | Does not close the main window | The window callback ignores Escape; closing asks for confirmation when a run is active. |

## Options if screen-reader support is required (approval needed before any change)

1. Keep FLTK-rs for factory and service stations, and make the CLI plus HTML reports, which screen readers can read, the accessible path for those users. This has the lowest effort.
2. Wait for or contribute FLTK accessibility work. Its upstream status should be monitored.
3. Move to a Rust toolkit with platform accessibility bridges (for example egui with AccessKit, or Slint, both of which expose UI Automation and AT-SPI). This would be a major UI rewrite. Visual fidelity to the approved industrial design would have to be re-validated.

No toolkit change has been made. Per CLAUDE.md, approval is required before changing the toolkit.
