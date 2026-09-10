# Testing

`bun run verify` runs frontend typecheck, lint, format, coverage (lib/components thresholds) and Rust fmt/clippy/tests when the MSVC linker is available.

Critical Rust coverage: session machine, retry classifier/backoff/deadline, retention, OBS mapping, WAV roundtrip, OpenRouter httpmock cases.

Frontend: overlay labels, duration formatting, shadcn-style primitives.

E2E WebdriverIO is specified for CI once `link.exe` (VS C++ tools) is installed on the machine.

Do not run live OpenRouter tests in default CI.
