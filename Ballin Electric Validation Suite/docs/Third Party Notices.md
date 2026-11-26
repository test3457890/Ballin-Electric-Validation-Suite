# Third party notices and license tracking

Status: license review performed from crate metadata on 2026-11-25. This is an engineering review, not legal advice. Obtain legal review before commercial distribution. **No project license has been selected**; the workspace is marked `LicenseRef-Proprietary-Unlicensed` and `publish = false`.

Reference product names (Prime95, FurMark, OCCT, PC-Doctor, PassMark, HWiNFO, AIDA64, CPU-Z, MemTest86 and others) identify behavioral inspiration only. No code, algorithms, binaries, icons or assets from those products are used, and no affiliation is implied. The CPU stress test is an original workload: Lucas-Lehmer residues and FMA matrix products with known answers.

## Native components compiled into the executables

| Component | How included | License | Obligations / review status |
| --- | --- | --- | --- |
| FLTK 1.4 (C++) | Compiled from source by `fltk-sys` and statically linked | FLTK License: LGPL-2.0 with exceptions that permit static linking without releasing application source | Credit FLTK in documentation ("This software is based in part on the work of the FLTK project", https://www.fltk.org). Modifications to FLTK itself must be made available. **No modifications made.** Review status: accepted pending legal confirmation. |
| fltk-rs (`fltk`, `fltk-sys`) | Rust bindings | MIT | Include notice. |
| SQLite | Bundled amalgamation via `libsqlite3-sys` (`bundled` feature) | Public domain | None. |
| MinGW-w64 runtime (Windows GNU builds only) | Statically linked CRT/libgcc/winpthreads | GCC Runtime Library Exception / MIT / ZPL | If MinGW builds are distributed, include their notices. Release builds should prefer the MSVC toolchain. **Not yet decided.** |

## Runtime-loaded or external software (not bundled)

| Software | Use | License | Status |
| --- | --- | --- | --- |
| Vulkan loader and GPU vendor Vulkan drivers (ICDs) | Loaded at runtime by `ash` (`loaded` feature, via libloading) for GPU tests | Loader: Apache-2.0 (Khronos). Drivers: vendor terms. | Not bundled or redistributed. A missing loader is reported as UNSUPPORTED. |
| glslang (`glslangValidator`), SPIRV-Tools (`spirv-val`) | Development-time only: compile the original GLSL in `crates/be-gpu/shaders` to the SPIR-V that is committed and embedded | glslang: BSD-3-Clause and others; SPIRV-Tools: Apache-2.0 | Not shipped. The output SPIR-V is compiled from our own shader source. |
| Mesa lavapipe (llvmpipe Vulkan) | CI and development testing only (a software Vulkan device) | MIT | Not shipped. Results on it never count as GPU hardware evidence. |
| NVIDIA Management Library (NVML) | Loaded at runtime from the installed NVIDIA driver via `nvml-wrapper` (MIT/Apache-2.0) | NVIDIA proprietary (driver EULA) | Not redistributed. Use is subject to the end user's installed driver terms. The wrapper license does not grant rights to NVML. |
| smartmontools `smartctl` | Optional; invoked as a separate process for JSON SMART output (BE-T-STO-002) | GPL-2.0-or-later | Not bundled or linked. If a future installer bundles smartctl, GPL distribution obligations apply (source offer) and require review. |
| systemd `journalctl` | Invoked to read the kernel log (BE-T-OS-001) | LGPL-2.1+ | OS component, not bundled. |
| Windows PowerShell / CIM | Invoked for Windows inventory | Windows component | OS component, not bundled. |

## Rust crate dependencies

79 third-party crates are compiled into at least one target (runtime and build dependencies, all platforms; dev-only test crates excluded). License expressions as declared by each crate:

| Count | License expression |
| --- | --- |
| 53 | MIT OR Apache-2.0 |
| 14 | MIT |
| 6 | MIT/Apache-2.0 |
| 1 | Apache-2.0 / MIT |
| 1 | Zlib |
| 1 | ISC |
| 1 | Unlicense OR MIT |
| 1 | (MIT OR Apache-2.0) AND Unicode-3.0 |
| 1 | Apache-2.0 OR MIT |

No copyleft (GPL/LGPL/MPL/AGPL) or unlicensed crates were found. All are permissive with attribution requirements: ship this list with the full license texts. Machine-readable manifest: [sbom/cargo-dependencies.json](sbom/cargo-dependencies.json). Automated checks on 2026-11-25: `cargo deny check` (cargo-deny 0.22.1, policy `deny.toml`) passed advisories, bans, licenses and sources, with duplicate-version warnings only. `cargo audit` reported no known vulnerabilities after upgrading rusqlite 0.37 → 0.43. Bundled SQLite via `libsqlite3-sys` 0.40 fixes RUSTSEC-2026-0459 / CVE-2026-119375, which affected 0.35.

| Crate | Version | License |
| --- | --- | --- |
| anstream | 1.0.0 | MIT OR Apache-2.0 |
| anstyle | 1.0.14 | MIT OR Apache-2.0 |
| anstyle-parse | 1.0.0 | MIT OR Apache-2.0 |
| anstyle-query | 1.1.5 | MIT OR Apache-2.0 |
| anstyle-wincon | 3.0.11 | MIT OR Apache-2.0 |
| anyhow | 1.0.105 | MIT OR Apache-2.0 |
| ash | 0.38.0+1.3.281 | MIT OR Apache-2.0 |
| bitflags | 2.13.2 | MIT OR Apache-2.0 |
| block-buffer | 0.10.4 | MIT OR Apache-2.0 |
| cc | 1.11.3 | MIT OR Apache-2.0 |
| cfg-if | 1.0.5 | MIT OR Apache-2.0 |
| clap | 4.6.10 | MIT OR Apache-2.0 |
| clap_builder | 4.6.10 | MIT OR Apache-2.0 |
| clap_derive | 4.6.9 | MIT OR Apache-2.0 |
| clap_lex | 1.1.2 | MIT OR Apache-2.0 |
| cmake | 0.1.58 | MIT OR Apache-2.0 |
| cmk | 0.1.2 | MIT |
| colorchoice | 1.0.5 | MIT OR Apache-2.0 |
| cpufeatures | 0.2.17 | MIT OR Apache-2.0 |
| crossbeam-channel | 0.5.17 | MIT OR Apache-2.0 |
| crossbeam-utils | 0.8.23 | MIT OR Apache-2.0 |
| crypto-common | 0.1.7 | MIT OR Apache-2.0 |
| darling | 0.20.11 | MIT |
| darling_core | 0.20.11 | MIT |
| darling_macro | 0.20.11 | MIT |
| digest | 0.10.7 | MIT OR Apache-2.0 |
| fallible-iterator | 0.3.0 | MIT/Apache-2.0 |
| fallible-streaming-iterator | 0.1.9 | MIT/Apache-2.0 |
| find-msvc-tools | 0.1.15 | MIT OR Apache-2.0 |
| fltk | 1.5.24 | MIT |
| fltk-sys | 1.5.24 | MIT |
| fnv | 1.0.7 | Apache-2.0 / MIT |
| foldhash | 0.2.0 | Zlib |
| generic-array | 0.14.7 | MIT |
| hashbrown | 0.17.2 | MIT OR Apache-2.0 |
| hashlink | 0.12.2 | MIT OR Apache-2.0 |
| heck | 0.5.0 | MIT OR Apache-2.0 |
| ident_case | 1.0.1 | MIT/Apache-2.0 |
| is_terminal_polyfill | 1.70.2 | MIT OR Apache-2.0 |
| itoa | 1.0.19 | MIT OR Apache-2.0 |
| libc | 0.2.191 | MIT OR Apache-2.0 |
| libloading | 0.8.9 | ISC |
| libsqlite3-sys | 0.40.0 | MIT |
| memchr | 2.8.3 | Unlicense OR MIT |
| minipaste | 0.1.0 | MIT |
| nvml-wrapper | 0.11.0 | MIT OR Apache-2.0 |
| nvml-wrapper-sys | 0.9.1 | MIT OR Apache-2.0 |
| once_cell | 1.21.4 | MIT OR Apache-2.0 |
| once_cell_polyfill | 1.70.2 | MIT OR Apache-2.0 |
| pkg-config | 0.3.34 | MIT OR Apache-2.0 |
| proc-macro2 | 1.0.107 | MIT OR Apache-2.0 |
| quote | 1.0.47 | MIT OR Apache-2.0 |
| rsqlite-vfs | 0.2.0 | MIT |
| rusqlite | 0.43.0 | MIT |
| serde | 1.0.229 | MIT OR Apache-2.0 |
| serde_core | 1.0.229 | MIT OR Apache-2.0 |
| serde_derive | 1.0.229 | MIT OR Apache-2.0 |
| serde_json | 1.0.154 | MIT OR Apache-2.0 |
| sha2 | 0.10.9 | MIT OR Apache-2.0 |
| shlex | 2.0.1 | MIT OR Apache-2.0 |
| smallvec | 1.16.6 | MIT OR Apache-2.0 |
| sqlite-wasm-rs | 0.6.3 | MIT |
| static_assertions | 1.1.0 | MIT OR Apache-2.0 |
| strsim | 0.11.1 | MIT |
| syn | 2.0.119 | MIT OR Apache-2.0 |
| syn | 3.0.10 | MIT OR Apache-2.0 |
| thiserror | 1.0.69 | MIT OR Apache-2.0 |
| thiserror | 2.0.21 | MIT OR Apache-2.0 |
| thiserror-impl | 2.0.21 | MIT OR Apache-2.0 |
| thiserror-impl | 1.0.69 | MIT OR Apache-2.0 |
| typenum | 1.20.1 | MIT OR Apache-2.0 |
| unicode-ident | 1.0.27 | (MIT OR Apache-2.0) AND Unicode-3.0 |
| utf8parse | 0.2.2 | Apache-2.0 OR MIT |
| vcpkg | 0.2.15 | MIT/Apache-2.0 |
| version_check | 0.9.5 | MIT/Apache-2.0 |
| windows-link | 0.2.1 | MIT OR Apache-2.0 |
| windows-sys | 0.61.2 | MIT OR Apache-2.0 |
| wrapcenum-derive | 0.4.1 | MIT/Apache-2.0 |
| zmij | 1.0.23 | MIT |

## Outstanding licensing items

- Select a project license and confirm ownership. **The owner must decide.**
- Have the FLTK static-linking terms and attribution reviewed by legal counsel.
- Decide between MSVC and MinGW for Windows release builds and collect the matching runtime notices.
- Generate a standards-format SBOM (CycloneDX or SPDX) in the release pipeline.
- Review licenses of any future GPU backend (Vulkan SDK loader, CUDA, ROCm/HIP) before integration. Wrapper crates do not grant SDK rights.
- Trademark clearance for the product name and the steel-sphere mark is not complete.
