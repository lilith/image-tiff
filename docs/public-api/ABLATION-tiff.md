# tiff (image-tiff fork) public-API ablation report

**Date:** 2026-06-11
**Snapshot commit:** 766c7ce
**Crate analyzed:** `tiff` (crate name = `tiff`, 2,351 items — lilith fork of image-rs/image-tiff)
**Grep template:** `ugrep -r --include="*.rs" --include="*.toml" "<symbol>" /home/lilith/work/ --exclude-dir=target --exclude-dir=.jj`

## Fork etiquette note

This is a fork of the upstream `image-rs/image-tiff` crate, published as `tiff` on crates.io. Per the mission brief: "inherited surface = noted not flagged." Only ZEN-ADDED leaks are in scope for flagging. Shade on upstream is not appropriate; this fork builds on a well-maintained upstream codebase.

## Consumer context

Primary consumer: `zentiff` (`/home/lilith/work/zen/zenextras/zentiff/`) — wraps `tiff` decode/encode via zencodec traits.
Secondary: `zenpipe/zencodecs` + `zenpipe/wasm-size-shim` reference `zentiff` as optional dep.
No direct `tiff::` API usage in org code outside zentiff.

## What the zen fork adds vs upstream

All zen commits are:
1. `feat: versioned public-API surface snapshots` — testing infra, no new pub items
2. `chore(tiff): trim spurious files from published package` — packaging
3. `deps(image-tiff): weezl 0.1.10 -> 0.2.1` — dep bump
4. Fuzz infra commits (zenutils-fuzz integration) — test-only
5. `fix: route WebP/Group3/JPEG reader allocations through Limits` — internal decoder fix, no new pub API
6. `fix: guard against shift overflow in TIFF decoder` — internal fix
7. `fix: prevent u16 overflow panic in Group3 fax run-length decode` — internal fix
8. `fix: enforce ifd_value_size limits and detect PackBits under-decode` — internal fix
9. `refactor: use typed slices for float predictor output` — internal refactor

**Net result: zero new public items added by the zen fork.** All 2,351 surface items are upstream-inherited.

## Summary

**0 items flagged for action.**

### Observations (informational, no action needed)

1. **Upstream-inherited surface (all 2,351 items)** — `tiff::decoder`, `tiff::decoder::ifd`, `tiff::encoder`, `tiff::encoder::colortype`, `tiff::encoder::compression`, `tiff::tags`. All are inherited from `image-rs/image-tiff`. Out of scope per fork etiquette.

2. **`EntryBytesReader`, `IfdDecoder`, `TiffCodingUnit`, `TiffHeader`** — These are upstream public types exposed for tag-level access (e.g., custom tag reading workflows). No org consumer uses them currently (zentiff doesn't need them for standard encode/decode). They are upstream-inherited; flagging them would be shading upstream design decisions, which is out of scope.

3. **No zen-added public items found.** The zen fork's additions are all internal bug fixes and test infrastructure. The public surface is 100% upstream-inherited.

4. **The fork is consumed via crates.io** — `zentiff` uses `tiff = { version = "0.11.3" }` which resolves to this published fork. The published crate is the primary consumer interface.

## Flagged items

| # | Item | Category | Proposal | Confidence |
|---|------|----------|----------|------------|
| — | (none) | — | — | — |

**0 flagged. 0 % of surface.**

## Digest

The zen image-tiff fork adds zero new public API items. All fixes are internal. The 2,351-item surface is inherited from upstream image-rs/image-tiff and is out of scope per fork etiquette. Primary org consumer (zentiff) uses the crate via crates.io. No leaks, no accidental exposures.
