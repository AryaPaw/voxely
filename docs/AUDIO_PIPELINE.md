# Audio pipeline

Canonical format: 48 kHz mono f32, then PCM16 WAV on disk.

Capture: cpal WASAPI. Callback only copies into `rtrb` and updates RMS/peak.

DSP after stop uses the active Voxely preset. Factory graphs: Fast dictation and Quality. Listen preview applies loudness matching between original and filtered playback without changing the STT buffer.

STT Optimized: High-pass 70 Hz -> RNNoise -> compressor -> soft expander -> gain +1.5 dB -> limiter. Gate off.

RNNoise implementation: `nnnoiseless` (permissive Rust port).

Retry always resends the processed file.
