# image-tiff (lilith fork) — Project Notes

## Investigation Notes

- **`gh` resolves to UPSTREAM here — always pass `-R lilith/image-tiff`.**
  This clone has `remote.upstream.gh-resolved base` set, so bare `gh run
  list`, `gh run view`, `gh pr create`, `gh issue` etc. silently target
  `image-rs/image-tiff` (the parent), NOT the fork. Diagnosed 2026-06-12:
  `gh run list --commit <fork-sha>` returned nothing and the visible "daily
  failing scheduled Fuzz" runs were upstream's (their matrix still asks for a
  `decode_image` bin their fuzz crate no longer declares). The fork's own CI
  on 793c7370 (api-doc migration) was green for both workflows — verified via
  explicit `gh api repos/lilith/image-tiff/actions/runs?event=push`.
  Extra caution: bare `gh pr/issue comment` here would post to a third-party
  org — that requires explicit user approval per global rules.
- Fork CI: `rust.yml` (Rust CI) + `fuzz.yml` both trigger on push to main and
  are active. Upstream divergence: fork main diverged long before upstream's
  2026-06-02 PR #394 merge; don't compare run histories across the two.
