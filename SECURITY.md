# Security policy

## Reporting

Email **danielnichiata@citeme.app** with a description and, ideally, an input
that reproduces the issue. Please don't open a public issue for anything you
believe is exploitable.

This is a single-maintainer project. Expect a first reply within a few days,
not within hours. There is no bug bounty.

## Supported versions

Only the latest published version (`citeme-engine-wasm` on npm) receives
fixes. There are no backports to older minors.

## What counts as a vulnerability here

This library parses untrusted input — bibliographies pasted or uploaded by
end users. The threat model is about what that input can do to a host
application:

- **A reachable panic.** The engine builds with `panic = "abort"` on
  `wasm32-unknown-unknown`, so a panic cannot be caught at the Wasm boundary —
  it poisons the instance and forces a re-instantiation. Any input that panics
  the parsers, normalizer, formatter, or exporters is a denial-of-service bug
  and is treated as a security issue — CSL-JSON passed to `formatOne` /
  `formatBatch` included. `crates/core/tests/no_panic_props.rs` exists to
  make this unreachable; a counterexample is a real finding.
- **Silent output corruption.** Malformed or adversarial input that produces
  syntactically valid but *wrong* BibTeX/BibLaTeX/RIS/YAML — for example
  characters that escape a field and inject structure into the exported file.
  Consumers reasonably trust exported files to round-trip; corruption that
  doesn't throw is worse than a crash, because no fallback fires.
  `crates/core/tests/export_validity.rs` guards this class.
- **Unbounded resource use.** Input that makes memory or CPU grow
  super-linearly past the documented `max_input_bytes` guard.

## What doesn't

- **Unescaped HTML in formatted output.** The HTML output format emits markup
  by design — CSL styles and CSL-JSON legitimately carry `<i>`, `<sub>`,
  `<span class="nocase">` — and the engine cannot tell that apart from markup
  an attacker placed in a title. Values are passed through verbatim, exactly
  as citeproc-js and citation-js do. Sanitizing is the consumer's job, or use
  the `plain` output format. Documented in the README; a tag allowlist is
  under consideration but is a contract change, not a patch.
- Rendering output you consider incorrect for a given CSL style. That's a
  correctness bug — open a normal issue.
- Calling the API in a way the documentation says will throw (unloaded locale,
  unknown style, malformed JSON). Expected failures return `Err`; see the
  error-semantics section of the README.
- Anything requiring the attacker to already control the host application.
