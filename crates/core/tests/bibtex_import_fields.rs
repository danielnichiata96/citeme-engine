//! BibTeX imports that came out wrong without saying so.
//!
//! Every case here imported with zero errors and wrong CSL-JSON — a dropped
//! month, a range's end as its date, a LaTeX command in a title, an entry
//! paired with another entry's fields — so no consumer-side fallback could
//! notice.

use citeme_engine_core::parsers::{bibtex::parse_bibtex, ParseOptions};
use serde_json::{json, Value};

fn only_entry(input: &str) -> Value {
    let result = parse_bibtex(input, &ParseOptions::default());
    assert_eq!(result.entries.len(), 1, "{:?}", result.errors);
    result.entries[0].clone()
}

fn article(fields: &str) -> Value {
    only_entry(&format!(
        "@article{{k, author = {{A, B}}, title = {{T}}, journal = {{J}}, {fields}}}"
    ))
}

#[test]
fn a_numeric_month_is_kept() {
    // biblatex's canonical month is a number; it was dropped silently.
    for month in ["month = {5}", "month = 5", "month = {05}"] {
        let e = article(&format!("year = {{2020}}, {month}"));
        assert_eq!(e["issued"]["date-parts"][0], json!([2020, 5]), "{month}");
    }
    let e = article("year = {2020}, month = {5}, day = {3}");
    assert_eq!(e["issued"]["date-parts"][0], json!([2020, 5, 3]));
}

#[test]
fn a_date_range_imports_as_its_start() {
    // hayagriva keeps the END of a biblatex range.
    let e = only_entry(
        "@inproceedings{k, author = {A, B}, title = {T}, booktitle = {B}, date = {2020/2021}}",
    );
    assert_eq!(e["issued"]["date-parts"][0], json!([2020]));
    let e = article("date = {2020-05/2020-06}");
    assert_eq!(e["issued"]["date-parts"][0], json!([2020, 5]));
}

#[test]
fn a_page_list_keeps_its_commas() {
    // hayagriva dropped the separators: "100-115200".
    let e = article("year = {2020}, pages = {100--115, 200}");
    assert_eq!(e["page"], "100-115, 200");
    let e = article("year = {2020}, pages = {100--115}");
    assert_eq!(e["page"], "100-115");
}

#[test]
fn and_others_is_not_an_author() {
    // Google Scholar truncates long author lists with "and others", which
    // came out as an author literally named "others".
    let e = only_entry(
        "@article{k, author = {Smith, John and others}, title = {T}, journal = {J}, year = {2020}}",
    );
    assert_eq!(e["author"], json!([{"family": "Smith", "given": "John"}]));
}

#[test]
fn latex_formatting_commands_become_their_text() {
    for (title, expected) in [
        (r"The \emph{Drosophila} genome", "The Drosophila genome"),
        (r"\textit{E. coli} growth", "E. coli growth"),
        (r"\textbf{Bold} claim", "Bold claim"),
        (
            r"CO\textsubscript{2} and x\textsuperscript{2}",
            "CO2 and x2",
        ),
        (
            r"{\it Old} style and {\em old} emphasis",
            "Old style and old emphasis",
        ),
        (r"See \url{https://x.org}", "See https://x.org"),
    ] {
        let e = article(&format!("year = {{2020}}, title = {{{title}}}"));
        assert_eq!(e["title"], expected, "{title}");
    }
}

#[test]
fn letter_commands_followed_by_a_group_become_letters() {
    // `\o{}` came out as a combining stroke on the previous letter:
    // "J̸rgensen".
    let e = only_entry(
        r"@article{k, author = {J\o{}rgensen, S\o{}ren and \O{}rsted, H}, title = {Stra\ss{}e \ss{x}}, journal = {J}, year = {2020}}",
    );
    assert_eq!(
        e["author"][0],
        json!({"family": "Jørgensen", "given": "Søren"})
    );
    assert_eq!(e["author"][1]["family"], "Ørsted");
    assert_eq!(e["title"], "Straße ßx");
}

#[test]
fn a_url_in_howpublished_or_note_becomes_the_url() {
    let e = only_entry(
        r"@misc{k, author = {A, B}, title = {T}, year = {2020}, howpublished = {\url{https://x.org/a}}}",
    );
    assert_eq!(e["URL"], "https://x.org/a");
    let e = only_entry(
        r"@misc{k, author = {A, B}, title = {T}, year = {2020}, note = {Available at \url{https://x.org/b}}}",
    );
    assert_eq!(e["URL"], "https://x.org/b");
    assert_eq!(e["note"], "Available at https://x.org/b");
    // An explicit `url` field wins.
    let e = only_entry(
        r"@misc{k, title = {T}, year = {2020}, url = {https://real.org/paper}, note = {\url{https://x.org}}}",
    );
    assert_eq!(e["URL"], "https://real.org/paper");
}

#[test]
fn a_year_that_is_not_a_date_keeps_the_entry() {
    // "in press" failed the date conversion, which rejected the entry and
    // sent the whole file down the per-chunk recovery path.
    let input = "@article{k, author = {A, B}, title = {Accepted}, journal = {J}, year = {in press}}\n\n@article{j, author = {C, D}, title = {Other}, journal = {J}, year = {2020}}";
    let result = parse_bibtex(input, &ParseOptions::default());
    assert_eq!(result.entries.len(), 2, "{:?}", result.errors);
    assert!(result.errors.is_empty(), "{:?}", result.errors);
    assert!(result.entries[0].get("issued").is_none());
    assert_eq!(result.entries[1]["issued"]["date-parts"][0], json!([2020]));
}

#[test]
fn a_year_that_is_a_publication_status_imports_as_the_status() {
    // Kept with no date, "in press" rendered "(n.d.)" in APA. CSL has a
    // variable for it — `status` — which styles print in the date's place.
    for (year, status) in [
        ("in press", "in press"),
        ("Forthcoming", "Forthcoming"),
        ("{In Press}", "In Press"),
    ] {
        let entry = article(&format!("year = {{{year}}}"));
        assert_eq!(entry["status"], status, "{entry}");
        assert!(entry.get("issued").is_none(), "{entry}");
    }
    // Only words: a broken numeric year is not a status.
    assert!(article("year = {20x0}").get("status").is_none());
}

#[test]
fn an_urldate_is_read_only_when_unambiguous() {
    let accessed = |urldate: &str| {
        let e = only_entry(&format!(
            "@online{{k, title = {{T}}, url = {{https://a.b}}, year = {{2020}}, urldate = {{{urldate}}}}}"
        ));
        e["accessed"]["date-parts"][0].clone()
    };
    // Read as year 15 before.
    assert_eq!(accessed("15/03/2024"), json!([2024, 3, 15]));
    assert_eq!(accessed("2024-03-15"), json!([2024, 3, 15]));
    assert_eq!(accessed("2024-03-15T10:00:00Z"), json!([2024, 3, 15]));
    // Day/month or month/day: not guessed.
    assert_eq!(accessed("05/03/2024"), Value::Null);
    // Out-of-range parts are not written.
    assert_eq!(accessed("2024-13-45"), json!([2024]));
}

#[test]
fn a_duplicate_key_keeps_its_own_fields_when_its_twin_is_rejected() {
    // Entries were paired with biblatex's by occurrence count; hayagriva
    // dropping one twin shifted the pairing, and the book inherited the
    // article's type and journal.
    let input = r#"@article{dup,
  author = {Smith, A},
  title = {Accepted article},
  journal = {Journal X},
  year = {in press}
}

@book{dup,
  author = {Doe, B},
  title = {A Book},
  publisher = {P},
  year = {2020}
}
"#;
    let result = parse_bibtex(input, &ParseOptions::default());
    assert_eq!(result.entries.len(), 2, "{:?}", result.errors);
    let book = &result.entries[1];
    assert_eq!(book["title"], "A Book");
    assert_eq!(book["type"], "book", "{book}");
    assert!(book.get("container-title").is_none(), "{book}");
    assert_eq!(book["id"], "dup", "the CSL id keeps the key as written");
    assert_eq!(result.entries[0]["type"], "article-journal");
}

#[test]
fn string_macros_still_resolve_when_a_key_is_duplicated() {
    // A duplicate key sent the file down the per-chunk recovery path, which
    // skipped @string chunks: every entry using a macro then failed.
    let input = r#"@string{jgr = {Journal of Geophysical Research}}

@article{a, author = {Smith, J}, title = {First}, journal = jgr, year = {2020}}

@article{b, author = {Doe, J}, title = {Second}, journal = jgr, year = {2021}}

@article{b, author = {Roe, R}, title = {Third}, journal = {Other}, year = {2022}}
"#;
    let result = parse_bibtex(input, &ParseOptions::default());
    assert!(result.errors.is_empty(), "{:?}", result.errors);
    let titles: Vec<&Value> = result.entries.iter().map(|e| &e["title"]).collect();
    assert_eq!(titles, [&json!("First"), &json!("Second"), &json!("Third")]);
    assert_eq!(
        result.entries[1]["container-title"],
        "Journal of Geophysical Research"
    );
}

#[test]
fn string_macros_still_resolve_around_a_broken_entry() {
    let input = r#"@string{nat = {Nature}}

@article{a, author = {Smith, J}, title = {Good one}, journal = nat, year = {2020}}

@article{broken, author = {Doe, J}, title = {Missing close brace, year = {2021}

@article{c, author = {Roe, R}, title = {Good two}, journal = nat, year = {2022}}
"#;
    let result = parse_bibtex(input, &ParseOptions::default());
    let titles: Vec<&Value> = result.entries.iter().map(|e| &e["title"]).collect();
    assert_eq!(
        titles,
        [&json!("Good one"), &json!("Good two")],
        "{:?}",
        result.errors
    );
    assert_eq!(result.entries[1]["container-title"], "Nature");
    assert_eq!(result.errors.len(), 1, "the broken entry is reported");
    assert_eq!(result.scanned_entries, 3);
}

#[test]
fn entries_sharing_a_line_survive_a_duplicate_key() {
    // CrossRef returns each entry on one line; pasted back to back, a
    // duplicate key made the recovery path split on "\n@" only, so the
    // whole paste was one chunk and every entry was lost.
    let input = " @article{Smith_2020, title={First paper}, volume={1}, journal={J A}, author={Smith, John}, year={2020}} @article{Smith_2020, title={Second paper}, volume={2}, journal={J B}, author={Smith, John}, year={2020}} @article{Doe_2019, title={Third paper}, journal={J C}, author={Doe, Jane}, year={2019}}";
    let result = parse_bibtex(input, &ParseOptions::default());
    assert_eq!(result.entries.len(), 3, "{:?}", result.errors);
    assert_eq!(result.entries[1]["volume"], "2");
}

#[test]
fn max_entries_holds_when_a_line_has_several_entries() {
    let input = "@article{a, title={A}, journal={J}, year={2020}} @article{b, title={B}, journal={J}, year={2020}} @article{c, title={C}, journal={J}, year={2020}}\n@article{broken, title={unclosed, year={2020}\n";
    let options = ParseOptions {
        max_entries: Some(1),
        ..Default::default()
    };
    let result = parse_bibtex(input, &options);
    assert_eq!(result.entries.len(), 1);
    assert!(result.truncated);
}

#[test]
fn an_undefined_macro_inside_a_string_definition_is_kept_as_text() {
    // An undefined macro used in a field is kept as its name; used inside an
    // @string it failed a whole-file parse per definition. Past eight, the
    // import read entries one by one: every entry using a broken macro was
    // dropped, and every entry lost its crossref inheritance.
    let mut input = String::new();
    for i in 0..10 {
        input.push_str(&format!("@string{{j{i} = undefined # {{ Journal {i}}}}}\n"));
    }
    input.push_str("@book{parent, title = {Parent Book}, publisher = {P}, year = {2020}}\n");
    for i in 0..10 {
        input.push_str(&format!(
            "@incollection{{c{i}, author = {{A, B}}, title = {{Chapter {i}}}, note = j{i}, crossref = {{parent}}}}\n"
        ));
    }
    let result = parse_bibtex(&input, &ParseOptions::default());
    assert_eq!(result.entries.len(), 11, "{:?}", result.errors);
    for chapter in &result.entries[1..] {
        assert_eq!(chapter["publisher"], "P", "inheritance lost: {chapter}");
        assert!(
            chapter["note"]
                .as_str()
                .is_some_and(|n| n.starts_with("undefined Journal")),
            "{chapter}"
        );
    }
}
