# Horizontal predictor: NEON prefix sum — MEASURED AND REJECTED — 2026-07-28

Platform: Apple Silicon (aarch64, NEON), darwin 25.5.0
Bench: `benches/predictor_share.rs` (zenbench)

## The candidate

`rev_hpredict_nsamp` / `one_byte_predict::<N>` (`src/decoder/mod.rs`) is a per-channel prefix
sum — `acc[c] += chunk[c]` — serial by construction. The classic log-shift trick vectorizes it:
for RGBA, 16 bytes is 4 pixels, so two lane shifts (by 1 and 2 pixels) plus adds produce the
intra-block prefix sum, then one broadcast add applies the running carry.

Prototyped against the current scalar loop, bit-identical output:

| row | scalar | NEON | speedup |
|---|---|---|---|
| 1920 px RGBA | 7.9 GB/s | 10.9 GB/s | 1.38× |
| 4096 px RGBA | 8.1 GB/s | 11.0 GB/s | 1.35× |

Throughput is capped by the loop-carried carry dependency between 16-byte blocks — each block's
result feeds the next — which is inherent to a prefix sum.

## Why it was rejected: the predictor is ~10% of decode

Decode of a 1920×1080 RGB8 deflate TIFF, encoded by this crate, predictor on vs off:

| content | Predictor::None | Predictor::Horizontal | compressed size (none → horizontal) |
|---|---|---|---|
| noisy | 18.5 ms | 17.4 ms | 5,328,861 → 2,610,255 B |
| smooth | 16.9 ms | 13.3 ms | 1,769,547 → 930,310 B |

Decode is **inflate-dominated** — 13–18 ms to produce 6.2 MB is ~350–450 MB/s, an order of
magnitude below the predictor's 7.9 GB/s. The predictor-on files decode *faster overall*
despite doing the extra predictor pass, because the predictor halves the compressed size and
inflate is the expensive part. That is the predictor doing its job.

Isolating the predictor: with deflate level 0 (stored blocks, so decompression is nearly free)
the same decode is 0.79 ms without the predictor and 2.3 ms with it — so the predictor pass
itself costs ~1.5 ms on 6.2 MB, matching the 4 GB/s the 3-sample path achieves.

**1.5 ms out of a 13.3–18.5 ms decode is ~8–11%. A 1.38× speedup on it saves ~0.4 ms, i.e.
~3% of decode.** That does not justify adding `archmage` + `magetypes` as dependencies to a
fork of a third-party crate, which is a real cost to every consumer of this fork and a
divergence to carry against upstream forever.

## If this is ever revisited

The lever that would change the verdict is inflate, not the predictor — it is 90% of decode
here. `zenflate` is the workspace DEFLATE implementation; whether this fork should use it
instead of `flate2` is a dependency/compatibility question, not a NEON one.

A faster predictor would also need the multi-block form (compute 4 vectors' intra-block prefix
sums independently, then apply cumulative carries) to get past the ~11 GB/s carry-chain limit
the simple version hits. That was not prototyped, because the share above makes it moot.

## Note on the fixtures

`tests/images/quad-lzw-compat.tiff` does not decode (`InvalidCode`) and is skipped loudly by
the bench rather than panicking mid-run. That is pre-existing and unrelated to this work — it
appears to be a deliberate compat/malformed fixture.
