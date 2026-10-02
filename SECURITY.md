# Security Policy

## Threat model

`mmdconv` parses **untrusted binary files** (GLB, VRM, FBX, DAE, PMX…) that
frequently come from third-party download sites. The attacker-controlled
input is the primary attack surface; the CLI itself is trusted code.

Assumptions:

* Input files may be malformed, truncated, hostile, or crafted to exploit
  parser bugs.
* Files may embed huge decompression bombs (textures), absurd counts
  (vertex/bone/material/morph counts), deeply nested JSON, and pathological
  index data.
* Output paths are user-supplied; we must not write outside them.

## Guarantees

1. **Memory safety by construction.** Pure-Rust core path; parsers use
   bounds-checked cursors and `Vec::get`-style access — no `unsafe` in
   parsing code. Malformed input yields `Err(MmdconvError)`, never a panic
   or UB. (Panics on malformed input are treated as security bugs.)
2. **Resource ceilings.** Count fields read from files are sanity-checked
   against remaining byte length before allocation (a count of 2³¹ vertices
   in a 4 KB file fails fast instead of OOM-ing). Texture decode uses
   `image`'s limits; JSON parsing is depth-limited.
3. **Path hygiene.** Textures are written only under `<output-dir>/<tex-dir>`
   with ASCII-safe sanitized filenames; embedded glTF names are stripped of
   separators and `..` components. PMX stores relative forward-slash paths.
   We never follow symlinks outside the output tree.
4. **No network, no subprocesses** in the core path. Optional external-tool
   importers (.blend via Blender, USD) are feature-gated, documented, and
   invoke tools strictly from PATH with fixed argv (no shell interpolation).
5. **Deterministic output** prevents TOCTOU-style surprise diffs in CI
   pipelines consuming our artifacts.

## Known residual risks

* Decompression bombs: a legitimately-sized multi-hundred-MB texture set can
  still exhaust RAM during conversion. Mitigation planned: `--max-memory`
  budget + streaming (tracked; see docs/STATUS.md).
* DoS via extreme-but-legal geometry (10⁷ vertices): slow, not unsafe.
* Fuzzing coverage today is limited to random-byte tests on the PMX reader;
  structured GLB/JSON fuzz harnesses are scheduled for M10 — until then,
  treat exotic inputs as "likely robust, unproven".

## Reporting a vulnerability

* **Do not open a public issue.** Email security@mmdconv.example (replace
  with the real address once the project has one) or use GitHub's
  *Report a vulnerability* private form on the repository Security tab.
* Include: affected version (`mmdconv --version`) or commit hash, OS/arch,
  minimal reproducer file (or bytes), and command line.
* We acknowledge within 72 hours, triage within 7 days, and ship fixes with
  a GHSA advisory + CHANGELOG entry crediting the reporter (with consent).

## Hardening checklist (for maintainers, per release)

- [ ] `cargo audit` clean or every RUSTSEC has a written exception.
- [ ] Random-byte + truncated-prefix fuzz run ≥ 10 min per parser target.
- [ ] Grep for new `unwrap()/expect()/panic!()` outside tests.
- [ ] Confirm allocation-before-validation guards on every new count field.
- [ ] Verify texture filename sanitization rejects `..`, absolute paths,
      drive letters, and reserved Windows names (`CON`, `NUL`, …).
