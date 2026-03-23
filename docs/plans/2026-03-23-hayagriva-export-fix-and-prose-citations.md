# Hayagriva Export Fix + Prose Citations Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Fix the Hayagriva YAML export to produce valid `serial-number:` nesting (bugfix), fix `yaml_str()` to quote numeric strings, and expose narrative/prose citations (`Smith (2024)`) through new wasm methods (zero breaking change).

**Architecture:** Task 1 fixes the export module — identifiers move from flat fields to nested `serial-number:`, `yaml_str()` learns to quote numeric strings, and new CSL-JSON fields (`genre`, `chapter-number`, `PMID`, `PMCID`) get mapped. Task 2 adds `prose` to `FormatOptions` in core, then exposes it via **separate wasm methods** `formatOneProse()` / `formatBatchProse()` so existing callers are unaffected.

**Tech Stack:** Rust, hayagriva 0.9 (`CitePurpose::Prose`, `CitationItem::kind()`), wasm-bindgen, serde_json

---

## Task 1: Fix Hayagriva YAML export — yaml_str quoting, serial-number nesting, new fields

### Task 1.1: Write failing test for yaml_str numeric quoting

**Files:**
- Modify: `crates/core/src/export/hayagriva.rs` (tests block)

**Step 1: Add test that asserts numeric strings get quoted**

Add this test at the bottom of the `mod tests` block in `crates/core/src/export/hayagriva.rs`:

```rust
#[test]
fn test_yaml_str_quotes_numeric_strings() {
    // Pure numeric strings must be quoted to prevent YAML interpreting as int/float
    assert_eq!(yaml_str("12345678"), "\"12345678\"");
    assert_eq!(yaml_str("3.14"), "\"3.14\"");
    assert_eq!(yaml_str("0"), "\"0\"");
    // Non-numeric strings should remain unquoted
    assert_eq!(yaml_str("PMC9876543"), "PMC9876543");
    assert_eq!(yaml_str("10.1234/test"), "10.1234/test");
    assert_eq!(yaml_str("978-0747551003"), "978-0747551003");
}
```

**Step 2: Run test to verify it fails**

Run: `cargo test -p citeme-engine-core test_yaml_str_quotes_numeric_strings -- --nocapture`
Expected: FAIL — `yaml_str("12345678")` returns `12345678` (unquoted).

### Task 1.2: Fix yaml_str to quote numeric strings

**Files:**
- Modify: `crates/core/src/export/hayagriva.rs:43-53`

**Step 1: Add numeric check to yaml_str**

Replace the `yaml_str` function (lines 43-53) with:

```rust
fn yaml_str(s: &str) -> String {
    if s.contains(':') || s.contains('#') || s.contains('"') || s.contains('\'')
        || s.contains('\n') || s.starts_with('{') || s.starts_with('[')
        || s.starts_with(' ') || s.ends_with(' ')
        || s.parse::<f64>().is_ok()
    {
        // Use double quotes with escaped internal quotes
        format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
    } else {
        s.to_string()
    }
}
```

**Step 2: Run test to verify it passes**

Run: `cargo test -p citeme-engine-core test_yaml_str_quotes_numeric -- --nocapture`
Expected: PASS

### Task 1.3: Write failing test for serial-number nesting

**Files:**
- Modify: `crates/core/src/export/hayagriva.rs` (tests block)

**Step 1: Add test that asserts correct serial-number YAML structure**

```rust
#[test]
fn test_export_hayagriva_serial_number() {
    let item = json!({
        "type": "article-journal",
        "id": "smith2024",
        "title": "A Study",
        "author": [{"family": "Smith", "given": "John"}],
        "issued": {"date-parts": [[2024]]},
        "DOI": "10.1234/test",
        "ISBN": "978-0747551003",
        "ISSN": "2412-3129",
        "PMID": "12345678",
        "PMCID": "PMC9876543"
    });

    let yaml = csl_json_to_hayagriva(&item);
    // Identifiers MUST be nested under serial-number:
    assert!(yaml.contains("  serial-number:"), "should have serial-number block: {yaml}");
    assert!(yaml.contains("    doi: 10.1234/test"), "doi nested under serial-number: {yaml}");
    assert!(yaml.contains("    isbn: 978-0747551003"), "isbn nested under serial-number: {yaml}");
    assert!(yaml.contains("    issn: 2412-3129"), "issn nested under serial-number: {yaml}");
    assert!(yaml.contains("    pmid: \"12345678\""), "pmid should be quoted (numeric): {yaml}");
    assert!(yaml.contains("    pmcid: PMC9876543"), "pmcid nested under serial-number: {yaml}");
    // Must NOT have flat doi/isbn/issn fields at root level
    assert!(!yaml.contains("\n  doi:"), "doi must not be flat: {yaml}");
    assert!(!yaml.contains("\n  isbn:"), "isbn must not be flat: {yaml}");
    assert!(!yaml.contains("\n  issn:"), "issn must not be flat: {yaml}");
}
```

**Step 2: Run test to verify it fails**

Run: `cargo test -p citeme-engine-core test_export_hayagriva_serial_number -- --nocapture`
Expected: FAIL — current code writes flat `doi:`, `isbn:`, `issn:` fields.

### Task 1.4: Write failing test for genre and chapter fields

**Files:**
- Modify: `crates/core/src/export/hayagriva.rs` (tests block)

**Step 1: Add test**

```rust
#[test]
fn test_export_hayagriva_genre_and_chapter() {
    let item = json!({
        "type": "thesis",
        "id": "lee2024",
        "title": "My Dissertation",
        "author": [{"family": "Lee", "given": "Alice"}],
        "issued": {"date-parts": [[2024]]},
        "genre": "Doctoral dissertation",
        "chapter-number": "3"
    });

    let yaml = csl_json_to_hayagriva(&item);
    assert!(yaml.contains("  genre: Doctoral dissertation"), "should have genre: {yaml}");
    assert!(yaml.contains("  chapter: \"3\""), "should have chapter (quoted numeric): {yaml}");
}
```

**Step 2: Run test to verify it fails**

Run: `cargo test -p citeme-engine-core test_export_hayagriva_genre_and_chapter -- --nocapture`
Expected: FAIL — current code doesn't map `genre` or `chapter-number`.

### Task 1.5: Implement serial-number nesting + new fields

**Files:**
- Modify: `crates/core/src/export/hayagriva.rs:142-154`

**Step 1: Replace the identifiers block (lines 142-146) with serial-number nesting**

Replace lines 142-146 (`// Identifiers` through ISSN) with:

```rust
    // Serial-number (identifiers nested under serial-number:)
    let mut serial_fields: Vec<String> = Vec::new();
    if let Some(v) = item["DOI"].as_str() { serial_fields.push(format!("    doi: {}", yaml_str(v))); }
    if let Some(v) = item["ISBN"].as_str() { serial_fields.push(format!("    isbn: {}", yaml_str(v))); }
    if let Some(v) = item["ISSN"].as_str() { serial_fields.push(format!("    issn: {}", yaml_str(v))); }
    if let Some(v) = item["PMID"].as_str() { serial_fields.push(format!("    pmid: {}", yaml_str(v))); }
    if let Some(v) = item["PMCID"].as_str() { serial_fields.push(format!("    pmcid: {}", yaml_str(v))); }
    if !serial_fields.is_empty() {
        lines.push("  serial-number:".to_string());
        lines.extend(serial_fields);
    }
```

Replace line 144 (`URL`) — keep it as a standalone field but move after serial-number:

```rust
    // URL (separate from serial-number)
    if let Some(v) = item["URL"].as_str() { lines.push(format!("  url: {}", yaml_str(v))); }
```

Then after the URL line and before `// Language` (line 148), add:

```rust
    // Genre
    if let Some(v) = item["genre"].as_str() { lines.push(format!("  genre: {}", yaml_str(v))); }

    // Chapter
    if let Some(v) = item["chapter-number"].as_str() { lines.push(format!("  chapter: {}", yaml_str(v))); }
```

**Step 2: Fix the existing test assertion**

In `test_export_hayagriva_article` (line 197), change:
```rust
        assert!(yaml.contains("doi: 10.1234/test"), "should have doi: {yaml}");
```
to:
```rust
        assert!(yaml.contains("    doi: 10.1234/test"), "should have doi under serial-number: {yaml}");
```

**Step 3: Run all export tests to verify they pass**

Run: `cargo test -p citeme-engine-core export::hayagriva -- --nocapture`
Expected: ALL PASS

### Task 1.6: Commit

```bash
git add crates/core/src/export/hayagriva.rs
git commit -m "fix: Hayagriva YAML export — nest identifiers under serial-number, quote numerics, add genre/chapter/PMID"
```

---

## Task 2: Expose narrative/prose citations via separate wasm methods (zero breaking change)

### Task 2.1: Add `prose` field to FormatOptions

**Files:**
- Modify: `crates/core/src/types.rs:15-30`

**Step 1: Add `prose` field to `FormatOptions`**

In `crates/core/src/types.rs`, replace the struct and Default impl (lines 15-30) with:

```rust
/// Options for formatting operations.
#[derive(Debug, Clone)]
pub struct FormatOptions {
    /// Output format for the formatted strings
    pub output_format: OutputFormat,
    /// Apply ABNT 2023 post-processing
    pub abnt_post_process: bool,
    /// Use prose/narrative citation form ("Smith (2024)" instead of "(Smith, 2024)")
    pub prose: bool,
}

impl Default for FormatOptions {
    fn default() -> Self {
        Self {
            output_format: OutputFormat::Html,
            abnt_post_process: false,
            prose: false,
        }
    }
}
```

**Step 2: Fix existing wasm callsites that use exhaustive struct literals**

The two existing `FormatOptions` constructions in `crates/wasm/src/lib.rs` (lines 54-57 and 76-79) do NOT use `..Default::default()`, so adding the `prose` field will cause `error[E0063]: missing field 'prose'`. Add `prose: false` to both:

In `crates/wasm/src/lib.rs` line 54-57, change:
```rust
        let options = FormatOptions {
            output_format: OutputFormat::Html,
            abnt_post_process,
        };
```
to:
```rust
        let options = FormatOptions {
            output_format: OutputFormat::Html,
            abnt_post_process,
            prose: false,
        };
```

In `crates/wasm/src/lib.rs` line 76-79, make the same change (add `prose: false`).

**Step 3: Verify everything compiles**

Run: `cargo check --workspace`
Expected: OK (all callsites now provide the `prose` field)

### Task 2.2: Write failing test for prose citation in engine

**Files:**
- Modify: `crates/core/src/engine.rs` (tests block)

**Step 1: Add failing tests**

Add at the bottom of the `mod tests` block in `crates/core/src/engine.rs`:

```rust
#[test]
fn test_format_one_prose_citation() {
    let mut engine = CitationEngine::new();
    engine.load_style("apa", SAMPLE_CSL).unwrap();
    engine.load_locale("en-US", SAMPLE_LOCALE).unwrap();

    let csl_json = r#"{
        "type": "article-journal",
        "id": "smith2024",
        "title": "A Study of Something",
        "author": [{"family": "Smith", "given": "John"}],
        "issued": {"date-parts": [[2024]]},
        "container-title": "Journal of Testing",
        "volume": "42"
    }"#;

    let prose_opts = FormatOptions {
        prose: true,
        ..Default::default()
    };
    let result = engine.format_one(csl_json, "apa", "en-US", &prose_opts).unwrap();

    // Prose citation: "Smith (2024)" — author outside parens, year inside
    assert!(!result.in_text.is_empty(), "prose in_text must not be empty");
    assert!(result.in_text.contains("Smith"), "prose in_text should contain author: {}", result.in_text);
    assert!(result.in_text.contains("2024"), "prose in_text should contain year: {}", result.in_text);
    // Positive: prose form starts with bare author name
    let stripped = result.in_text.replace("<span", "").replace("</span>", "");
    assert!(!stripped.trim_start().starts_with('('),
        "prose in_text should start with bare author, not '(' — got: {}", result.in_text);
}

#[test]
fn test_format_batch_prose_citation() {
    let mut engine = CitationEngine::new();
    engine.load_style("apa", SAMPLE_CSL).unwrap();
    engine.load_locale("en-US", SAMPLE_LOCALE).unwrap();

    let csl_json_array = r#"[
        {
            "type": "article-journal",
            "id": "smith2024",
            "title": "First Paper",
            "author": [{"family": "Smith", "given": "John"}],
            "issued": {"date-parts": [[2024]]}
        },
        {
            "type": "book",
            "id": "doe2023",
            "title": "A Great Book",
            "author": [{"family": "Doe", "given": "Jane"}],
            "issued": {"date-parts": [[2023]]}
        }
    ]"#;

    let prose_opts = FormatOptions {
        prose: true,
        ..Default::default()
    };
    let results = engine.format_batch(csl_json_array, "apa", "en-US", &prose_opts).unwrap();

    assert_eq!(results.len(), 2);
    // Both must be non-empty
    assert!(!results[0].in_text.is_empty(), "first prose citation must not be empty");
    assert!(!results[1].in_text.is_empty(), "second prose citation must not be empty");
    // Positive: contains author names
    assert!(results[0].in_text.contains("Smith"),
        "first prose citation should contain Smith: {}", results[0].in_text);
    assert!(results[1].in_text.contains("Doe"),
        "second prose citation should contain Doe: {}", results[1].in_text);
    // Negative: should not be parenthetical form
    let s0 = results[0].in_text.replace("<span", "").replace("</span>", "");
    let s1 = results[1].in_text.replace("<span", "").replace("</span>", "");
    assert!(!s0.trim_start().starts_with('('),
        "first prose citation should not be parenthetical: {}", results[0].in_text);
    assert!(!s1.trim_start().starts_with('('),
        "second prose citation should not be parenthetical: {}", results[1].in_text);
}
```

**Step 2: Run tests to verify they fail**

Run: `cargo test -p citeme-engine-core test_format_one_prose_citation test_format_batch_prose_citation -- --nocapture`
Expected: FAIL — engine ignores `options.prose` (no `CitePurpose` threading yet).

### Task 2.3: Implement prose support in engine.rs

**Files:**
- Modify: `crates/core/src/engine.rs:4` (imports)
- Modify: `crates/core/src/engine.rs:113` (format_one CitationItem)
- Modify: `crates/core/src/engine.rs:207` (format_batch CitationItem)

**Step 1: Add `CitePurpose` to imports**

In `crates/core/src/engine.rs` line 4, change:

```rust
use hayagriva::{BibliographyDriver, BibliographyRequest, BufWriteFormat, CitationItem, CitationRequest};
```

to:

```rust
use hayagriva::{BibliographyDriver, BibliographyRequest, BufWriteFormat, CitationItem, CitationRequest, CitePurpose};
```

**Step 2: Use CitePurpose::Prose in format_one**

In `crates/core/src/engine.rs`, replace line 113:

```rust
        let cite_items = vec![CitationItem::with_entry(&item)];
```

with:

```rust
        let mut cite_item = CitationItem::with_entry(&item);
        if options.prose {
            cite_item = cite_item.kind(CitePurpose::Prose);
        }
        let cite_items = vec![cite_item];
```

**Step 3: Use CitePurpose::Prose in format_batch**

In `crates/core/src/engine.rs`, replace line 207:

```rust
            let cite_items = vec![CitationItem::with_entry(item)];
```

with:

```rust
            let mut cite_item = CitationItem::with_entry(item);
            if options.prose {
                cite_item = cite_item.kind(CitePurpose::Prose);
            }
            let cite_items = vec![cite_item];
```

**Step 4: Run tests to verify they pass**

Run: `cargo test -p citeme-engine-core test_format_one_prose test_format_batch_prose -- --nocapture`
Expected: PASS

**Step 5: Run ALL engine tests to check no regressions**

Run: `cargo test -p citeme-engine-core -- --nocapture`
Expected: ALL PASS

### Task 2.4: Add separate wasm methods (zero breaking change)

**Files:**
- Modify: `crates/wasm/src/lib.rs` (add methods after existing formatOne/formatBatch)

**Step 1: Add `formatOneProse` and `formatBatchProse` methods**

Add these methods inside the `impl WasmCitationEngine` block, after the existing `formatOne` method (after line 86):

```rust
    /// Format a single CSL-JSON item as a narrative/prose citation ("Smith (2024)").
    /// Input: JSON string of one CSL-JSON item. Output: JSON string of FormatResult.
    #[wasm_bindgen(js_name = "formatOneProse")]
    pub fn format_one_prose(
        &self,
        csl_json_str: &str,
        style_name: &str,
        locale_code: &str,
        abnt_post_process: bool,
    ) -> Result<String, JsError> {
        let options = FormatOptions {
            output_format: OutputFormat::Html,
            abnt_post_process,
            prose: true,
        };

        let result = self.inner.format_one(csl_json_str, style_name, locale_code, &options)
            .map_err(|e| JsError::new(&e.to_string()))?;

        serde_json::to_string(&result)
            .map_err(|e| JsError::new(&e.to_string()))
    }

    /// Format a batch of CSL-JSON items as narrative/prose citations ("Smith (2024)").
    /// Input: JSON string of CSL-JSON array. Output: JSON string of FormatResult array.
    #[wasm_bindgen(js_name = "formatBatchProse")]
    pub fn format_batch_prose(
        &self,
        csl_json_str: &str,
        style_name: &str,
        locale_code: &str,
        abnt_post_process: bool,
    ) -> Result<String, JsError> {
        let options = FormatOptions {
            output_format: OutputFormat::Html,
            abnt_post_process,
            prose: true,
        };

        let results = self.inner.format_batch(csl_json_str, style_name, locale_code, &options)
            .map_err(|e| JsError::new(&e.to_string()))?;

        serde_json::to_string(&results)
            .map_err(|e| JsError::new(&e.to_string()))
    }
```

**Step 2: Verify wasm crate compiles**

Run: `cargo check -p citeme-engine-wasm`
Expected: OK (no errors)

### Task 2.5: Update TypeScript types

**Files:**
- Modify: `js/index.d.ts`

**Step 1: Add new method declarations**

Add after the existing `formatOne` line (line 22) in `js/index.d.ts`:

```typescript
  formatOneProse(cslJsonStr: string, styleName: string, localeCode: string, abntPostProcess: boolean): string;
  formatBatchProse(cslJsonStr: string, styleName: string, localeCode: string, abntPostProcess: boolean): string;
```

### Task 2.6: Run full test suite

Run: `cargo test --workspace -- --nocapture`
Expected: ALL PASS

### Task 2.7: Commit

```bash
git add crates/core/src/types.rs crates/core/src/engine.rs crates/wasm/src/lib.rs js/index.d.ts
git commit -m "feat: add formatOneProse/formatBatchProse for narrative citations (Smith (2024))"
```

---

## Task 3: Version bump

### Task 3.1: Bump versions

**Files:**
- Modify: `crates/core/Cargo.toml` (version field)
- Modify: `crates/wasm/Cargo.toml` (version field)
- Modify: `js/package.json` (version field)

**Step 1: Bump all three files from `0.1.0` to `0.2.0`**

This is a minor release: bugfix (serial-number) + new feature (prose methods), no breaking changes.

- `crates/core/Cargo.toml`: `version = "0.2.0"`
- `crates/wasm/Cargo.toml`: `version = "0.2.0"` (also update core dependency version if pinned)
- `js/package.json`: `"version": "0.2.0"`

### Task 3.2: Commit

```bash
git add crates/core/Cargo.toml crates/wasm/Cargo.toml js/package.json
git commit -m "chore: bump version to 0.2.0"
```

---

## Summary

| Task | Type | Files Changed | Risk | Breaking? |
|------|------|---------------|------|-----------|
| 1.1-1.6 | Bugfix | `crates/core/src/export/hayagriva.rs` | Low — export-only, no format logic | No |
| 2.1-2.7 | Feature | `types.rs`, `engine.rs`, `wasm/lib.rs`, `js/index.d.ts` | Low — new methods, existing untouched | **No** |
| 3.1-3.2 | Chore | `Cargo.toml` ×2, `package.json` | None | No |

**Zero breaking changes.** Existing `formatOne()` / `formatBatch()` signatures are untouched. New `formatOneProse()` / `formatBatchProse()` methods are additive. citeme.app can adopt prose citations at its own pace.
