# Audio pipeline

Canonical format: 48 kHz mono f32, then PCM16 WAV on disk.

Capture: cpal WASAPI. Callback only copies into `rtrb` and updates RMS/peak.

DSP after stop:

OBS Imported (from this machine): Expander -> RNNoise -> Gate(off) -> Compressor -> Gain(+2 dB) -> Limiter(-1 dB). EQ is unsupported.

STT Optimized: High-pass 70 Hz -> RNNoise -> compressor -> soft expander -> gain +1.5 dB -> limiter. Gate off.

RNNoise implementation: `nnnoiseless` (permissive Rust port). Not a bitwise OBS match.

Retry always resends the processed file.
