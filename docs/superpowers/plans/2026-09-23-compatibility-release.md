# Compatibility and Release v0.5.0

The roadmap asks for a Windows 10/11, DPI, multiple-monitor, sleep/resume, and Explorer compatibility matrix; resource measurements; and release-content verification. A checked configuration must record the OS build, monitor layout, DPI, exact probe, result, and limitation. Unavailable hardware or disruptive transitions remain explicitly unverified rather than inferred from unit tests.

The release remains a portable x64 Windows application. Package creation must use Cargo's version as the single source of truth, create an isolated versioned directory and archive, include the current README, validation report and all applicable licenses, and verify every packaged file against SHA-256. CI must publish only the current archive.

Resource measurement starts only its own test process with an isolated config, refuses to report success if the single-instance mutex prevents startup, and records elapsed time, CPU time, memory and handle peaks. It must never stop a process it did not create.

- [x] Add reproducible release packaging and integrity verification, with a negative corruption check.
- [x] Add a read-only compatibility inventory and bounded resource measurement for the local desktop.
- [x] Exercise available monitor/DPI/AppBar and runtime probes; record Windows 10/11, sleep/resume and Explorer coverage honestly.
- [x] Build v0.5.0, verify package, test its executable, document results and remaining manual checks.
