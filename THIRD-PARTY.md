# Third-party notices

The controls/message-loop pattern, palette and font integration, PDH wrapper, and D3DKMT GPU wrapper are adapted from MH Monitoring, copyright (c) 2026 midnightheartsgames, MIT. The MIT license is reproduced in LICENSE.

Cuprum Regular and Bold are embedded unchanged. License: SIL Open Font License 1.1, reproduced in assets/fonts/OFL.txt (also distributed as Cuprum-OFL.txt in the portable release).

Rust dependencies retain their respective licenses. Cargo.lock records exact resolved versions; `cargo metadata --locked` lists their license expressions. Principal dependencies include egui/eframe (MIT OR Apache-2.0), serde/serde_json (MIT OR Apache-2.0), windows-sys (MIT OR Apache-2.0), sysinfo (MIT), nvml-wrapper (MIT OR Apache-2.0), tray-icon and global-hotkey (MIT OR Apache-2.0), and image (MIT OR Apache-2.0).

NVIDIA NVML is loaded from the user's NVIDIA driver installation; no NVML DLL is redistributed. The PawnIO modules AMDFamily17.bin and IntelMSR.bin 0.2.11 (namazso, LGPL-2.1-or-later) are embedded unchanged; provenance, hashes and license are in assets/pawnio/NOTICE.md and assets/pawnio/COPYING. The PawnIO driver itself is not redistributed: the user installs it with the official installer, and MH Sidebar talks to it only through its device IOCTL interface (no linking with PawnIOLib.dll). The PawnIO protocol and CPU sensor decoding are adapted from MH Monitoring. No WinRing0, LibreHardwareMonitor, OpenHardwareMonitor or PresentMon binary is included in MH Sidebar.

The supplied MH Sidebar cover is used as user-provided branding; it is not sourced from SidebarDiagnostics.
