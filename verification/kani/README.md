# Kani proofs for binary parsing kernels

These harnesses model-check the private functions used by production binary
decoding. They intentionally make no whole-parser or current-macOS equivalence
claim. The exact domains and exclusions are listed in `../README.md`.

## Reproduce

The recorded verifier version is Kani 0.67.0. Install it and its bundled model
checker, then run the checked-in wrapper from the repository root:

```sh
cargo install --locked kani-verifier --version 0.67.0
cargo kani setup
verification/kani/run.sh
```

Kani discovers every `#[kani::proof]` harness under `cfg(kani)`. To isolate one
property, pass its unique function name through the wrapper:

```sh
verification/kani/run.sh \
  --harness wide_be_u64_is_the_low_half_of_the_exact_value
```

Ordinary Cargo builds do not download or link Kani. Kani injects its support
crate only when `cargo kani` sets `cfg(kani)`.

## Trusted boundary

The proof trusts Kani 0.67.0, its Rust compiler and CBMC translation, and the
64-bit target arithmetic model. The correspondence between the executable
specifications and the historical C branches is reviewable rather than
machine-checked. In the pinned file, the relevant source is `_getSizedInt`
(approximately lines 735-755) and the format-`00` integer branch
(approximately lines 1100-1122).

The reference file itself is pinned by repository commit and SHA-256 in
`../reference.lock`; it is not fetched or executed by this proof run.

## Recorded run

On 2026-08-22, the wrapper completed with Kani 0.67.0 and CBMC 6.8.0 on
64-bit arm64 macOS:

```text
Manual Harness Summary:
Complete - 6 successfully verified harnesses, 0 failures, 6 total.
```

The verifier and its pinned nightly toolchain were installed under a temporary
directory for that run and removed afterward. The GitHub Actions workflow
repeats the same command on every pull request.
