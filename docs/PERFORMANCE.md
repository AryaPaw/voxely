# Performance

Targets:

- Idle: no polling except overlay meter while recording (32 ms)
- Overlay show: measure via `overlay_timing`
- DSP: Criterion bench `dsp_stt_1s`

This file will be filled with machine measurements after a release build on a machine with MSVC.

Current development machine lacked `link.exe` at first compile (empty VS 2022 directories), so native benches were not executed in that environment.
