//! A pre-scan of BibTeX input that defuses what the `biblatex` crate can't
//! survive.
//!
//! `biblatex` (and hayagriva's conversion on top of it) panics or recurses
//! without bound on input anyone can paste, and in Wasm a panic or a stack
//! overflow aborts the instance — `panic = "abort"`, no unwinding, nothing
//! for a consumer's try/catch to catch. The crate is not ours to patch, so
//! this module reads the raw syntax first with `biblatex::RawBibliography`
//! (iterative, linear) and rewrites the source before anything recursive
//! sees it:
//!
//! - date values the date parser can't take: non-ASCII digits (`²²`,
//!   `٢٠`), a sign followed by a space (`- 2020`), a day past 255 in
//!   `month` (`may 300`), a numeric month outside 1–12 — `unwrap` on a
//!   failed integer parse, or an integer that underflows;
//! - `crossref`/`xdata` cycles, and inheritance trees past a small size —
//!   resolution recurses through them without a visited set, and copies
//!   every ancestor for every descendant;
//! - `@string` macro cycles, deep chains and oversized expansions —
//!   resolution recurses, and `a = b # b` doubles per level;
//! - LaTeX command arguments nested past a depth a 1 MiB stack holds, and
//!   braces nested past a count where chunk flattening turns quadratic.
//!
//! A defused field or macro is reported in `errors`; the entry is kept.
//! Duplicate keys are renamed so one parse reads every entry, and the
//! readable chunks of a file with a syntax error are parsed together, so
//! their `@string` macros still resolve.
//!
//! It also corrects, at the source, LaTeX `biblatex` reads wrong: `\o{}`
//! (read as a combining stroke, not "ø") and formatting commands such as
//! `\emph{…}` and `{\it …}`, which it leaves in the text.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::ops::Range;

use biblatex::{Bibliography, ParseError, ParseErrorKind, RawBibliography, RawChunk};

use super::{ParseErrorInfo, MAX_PARSE_ERRORS};

/// Deepest nesting of LaTeX command arguments let through. `biblatex` parses
/// each argument with a recursive call; ~5,000 levels overflow the 1 MiB
/// Wasm stack. Real titles nest two or three.
const MAX_COMMAND_DEPTH: usize = 32;

/// Braces opened or closed inside an already-braced group, per value. Each
/// one starts a new verbatim chunk, and `biblatex` merges adjacent chunks
/// back with `Vec::remove` — quadratic: 40,000 of them took 7 s.
const MAX_INNER_BRACES: usize = 256;

/// Largest `crossref`/`xdata` inheritance tree: an entry plus every entry it
/// resolves, transitively, counted with repetition. Resolution walks (and
/// clones) the whole tree again for every entry in it.
const MAX_INHERITANCE: usize = 16;

/// Longest `@string` chain (a macro that uses a macro that …). Resolution
/// recurses once per link.
const MAX_MACRO_DEPTH: usize = 16;

/// The longest text one `@string` macro may expand to.
const MAX_MACRO_LEN: usize = 64 * 1024;

/// Whole-file re-parses allowed after the pre-scan, each defusing what the
/// previous one stopped on, before entries are parsed one by one. A safety
/// net: the pre-scan removes every failure it can predict.
const MAX_RETRIES: usize = 8;

/// What replaces a defused field: an unknown field `biblatex` keeps and
/// nothing downstream reads.
const DROPPED_FIELD: &str = "citemedropped = {}";

/// The parsed entries and what the pre-scan found on the way.
pub(super) struct Guarded {
    /// In file order, inheritance resolved.
    pub parsed: Vec<biblatex::Entry>,
    pub errors: Vec<ParseErrorInfo>,
    /// Per entry, by its key in `parsed`.
    pub notes: HashMap<String, EntryNotes>,
    /// Entries found, readable or not.
    pub scanned: usize,
}

pub(super) struct EntryNotes {
    /// The key as written. A duplicate key is renamed for the parse.
    pub key: String,
    /// A URL given only as `\url{…}`/`\href{…}` inside `howpublished` or
    /// `note`.
    pub url: Option<String>,
    /// The start of the entry's source, for error previews.
    pub preview: String,
}

/// Read `input` with everything that could crash or hang the parse defused.
pub(super) fn parse(input: &str) -> Guarded {
    let mut guard = Guard::read(input);
    guard.check_macros();
    guard.check_entries();
    guard.check_inheritance();
    guard.parse()
}

/// One piece of a field value: braced/quoted/number text, or a macro name.
#[derive(Clone)]
enum Part {
    Text(Range<usize>),
    Macro(String, Range<usize>),
}

struct Field {
    /// Lowercased, as `biblatex` stores it.
    key: String,
    /// From the start of the key to the end of the value.
    span: Range<usize>,
    parts: Vec<Part>,
}

struct Entry {
    key: String,
    key_span: Range<usize>,
    span: Range<usize>,
    fields: Vec<Field>,
    /// The key it parses under: `key`, or a fresh one for a duplicate.
    parse_key: String,
    url: Option<String>,
    /// Indices of fields already replaced by `DROPPED_FIELD`.
    dropped: HashSet<usize>,
    /// Excluded from the parse altogether (only after a failed re-parse).
    removed: bool,
}

struct MacroDef {
    name: String,
    span: Range<usize>,
    /// The value, from its first part to its last.
    value: Range<usize>,
    parts: Vec<Part>,
}

/// What one `@string` name expands to, once checked.
#[derive(Clone, Default)]
struct MacroInfo {
    /// Expansion length, saturating at `MAX_MACRO_LEN + 1`.
    len: usize,
    /// The expansion as plain text, when short enough to inspect.
    text: Option<String>,
}

struct Guard<'a> {
    input: &'a str,
    entries: Vec<Entry>,
    macros: Vec<MacroDef>,
    macro_info: HashMap<String, MacroInfo>,
    /// Replacements in `input`, by start offset; never overlapping.
    patches: BTreeMap<usize, (usize, String)>,
    errors: Vec<ParseErrorInfo>,
    scanned: usize,
}

impl<'a> Guard<'a> {
    /// Collect entries and `@string` definitions with their spans. A file
    /// that doesn't parse as a whole is split at every line that opens with
    /// `@`; the chunks that parse are kept, and the rest reported.
    fn read(input: &'a str) -> Self {
        let mut guard = Guard {
            input,
            entries: Vec::new(),
            macros: Vec::new(),
            macro_info: HashMap::new(),
            patches: BTreeMap::new(),
            errors: Vec::new(),
            scanned: 0,
        };

        if let Ok(raw) = RawBibliography::parse(input) {
            guard.collect(raw, 0);
            return guard;
        }

        let starts = chunk_starts(input);
        for (i, &start) in starts.iter().enumerate() {
            let end = starts.get(i + 1).copied().unwrap_or(input.len());
            let chunk = &input[start..end];
            match RawBibliography::parse(chunk) {
                Ok(raw) => guard.collect(raw, start),
                Err(e) => {
                    let kind = chunk_kind(chunk);
                    let kind = kind.as_deref();
                    if matches!(kind, Some("comment" | "preamble")) {
                        // Neither carries data; a broken one costs nothing.
                    } else {
                        if kind.is_some_and(|k| k != "string") {
                            guard.scanned += 1;
                        }
                        guard.report(chunk, format!("biblatex parse error: {}", e.kind));
                    }
                    guard.patches.insert(start, (end, String::new()));
                }
            }
        }
        guard
    }

    fn collect(&mut self, raw: RawBibliography<'_>, offset: usize) {
        let at = |span: &Range<usize>| span.start + offset..span.end + offset;
        let parts = |value: &biblatex::Field<'_>| -> Vec<Part> {
            value
                .iter()
                .map(|chunk| match chunk.v {
                    RawChunk::Normal(_) => Part::Text(at(&chunk.span)),
                    RawChunk::Abbreviation(name) => Part::Macro(name.to_string(), at(&chunk.span)),
                })
                .collect()
        };

        for pair in &raw.abbreviations {
            let value = at(&pair.value.span);
            self.macros.push(MacroDef {
                name: pair.key.v.to_string(),
                span: at(&pair.key.span).start..value.end,
                value,
                parts: parts(&pair.value.v),
            });
        }

        for entry in &raw.entries {
            self.scanned += 1;
            let fields = entry
                .v
                .fields
                .iter()
                .map(|pair| Field {
                    key: pair.key.v.to_ascii_lowercase(),
                    span: at(&pair.key.span).start..at(&pair.value.span).end,
                    parts: parts(&pair.value.v),
                })
                .collect();
            self.entries.push(Entry {
                key: entry.v.key.v.to_string(),
                key_span: at(&entry.v.key.span),
                span: at(&entry.span),
                fields,
                parse_key: entry.v.key.v.to_string(),
                url: None,
                dropped: HashSet::new(),
                removed: false,
            });
        }
    }

    fn report(&mut self, source: &str, error: String) {
        if self.errors.len() < MAX_PARSE_ERRORS {
            self.errors.push(ParseErrorInfo {
                preview: source.trim().chars().take(80).collect(),
                error,
            });
        }
    }

    /// Replace `range` of the input. A patch inside an earlier, wider one
    /// is moot; earlier patches inside this one are superseded.
    fn patch(&mut self, range: Range<usize>, with: String) {
        if let Some((&start, &(end, _))) = self.patches.range(..=range.start).next_back() {
            if start <= range.start && range.end <= end && (start, end) != (range.start, range.end)
            {
                return;
            }
        }
        let inner: Vec<usize> = self
            .patches
            .range(range.start..range.end.max(range.start + 1))
            .filter(|(_, (end, _))| *end <= range.end)
            .map(|(&start, _)| start)
            .collect();
        for start in inner {
            self.patches.remove(&start);
        }
        self.patches.insert(range.start, (range.end, with));
    }

    fn drop_field(&mut self, entry: usize, field: usize, why: &str) {
        let e = &mut self.entries[entry];
        if e.removed || !e.dropped.insert(field) {
            return;
        }
        let (span, key) = (e.fields[field].span.clone(), e.fields[field].key.clone());
        let preview = self.input[e.span.clone()].to_string();
        self.patch(span, DROPPED_FIELD.to_string());
        self.report(&preview, format!("{key}: {why} — imported without it"));
    }

    /// Check every `@string` name: its definitions must parse, must not
    /// reach themselves, and must expand to something bounded. A failing
    /// name is emptied in place, so uses of it read as "".
    fn check_macros(&mut self) {
        let mut by_name: HashMap<String, Vec<usize>> = HashMap::new();
        for (i, def) in self.macros.iter().enumerate() {
            by_name.entry(def.name.clone()).or_default().push(i);
        }

        // Bottom-up: a name is ready once every name it uses is. Names left
        // over are on a cycle or use one.
        let mut uses: HashMap<String, HashSet<String>> = HashMap::new();
        let mut users: HashMap<String, Vec<String>> = HashMap::new();
        for (name, defs) in &by_name {
            let mut refs = HashSet::new();
            for &d in defs {
                for part in &self.macros[d].parts {
                    if let Part::Macro(r, _) = part {
                        if by_name.contains_key(r) {
                            refs.insert(r.clone());
                        }
                    }
                }
            }
            for r in &refs {
                users.entry(r.clone()).or_default().push(name.clone());
            }
            uses.insert(name.clone(), refs);
        }
        let mut pending: HashMap<String, usize> =
            uses.iter().map(|(n, r)| (n.clone(), r.len())).collect();
        let mut ready: Vec<String> = pending
            .iter()
            .filter(|(_, &n)| n == 0)
            .map(|(name, _)| name.clone())
            .collect();
        let mut depth: HashMap<String, usize> = HashMap::new();

        while let Some(name) = ready.pop() {
            let defs = by_name[&name].clone();
            let mut info = MacroInfo::default();
            let mut d = 1;
            let mut fault = None;
            let mut text = Some(String::new());
            for &def in &defs {
                let mut len = 0usize;
                let mut def_text = Some(String::new());
                for part in self.macros[def].parts.clone() {
                    let (part_len, part_text) = match &part {
                        Part::Text(range) => {
                            let raw = &self.input[range.clone()];
                            match check_content(raw, false) {
                                Ok(stats) if stats.inner_braces > MAX_INNER_BRACES => {
                                    fault = Some("too many nested braces".to_string());
                                }
                                Ok(_) => {}
                                Err(why) => fault = Some(why.to_string()),
                            }
                            (raw.len(), Some(plain_text(raw)))
                        }
                        Part::Macro(r, _) => match self.macro_info.get(r) {
                            Some(used) => {
                                d = d.max(depth.get(r).copied().unwrap_or(0) + 1);
                                (used.len, used.text.clone())
                            }
                            None => {
                                let word = month_name(r).map_or(r.as_str(), |m| m);
                                (word.len(), Some(word.to_string()))
                            }
                        },
                    };
                    len = len.saturating_add(part_len);
                    def_text = match (def_text, part_text) {
                        (Some(mut t), Some(p)) if t.len() + p.len() <= 4096 => {
                            t.push_str(&p);
                            Some(t)
                        }
                        _ => None,
                    };
                }
                info.len = info.len.max(len);
                // Duplicate definitions: biblatex 0.11 takes the first,
                // 0.12 the last. Inspecting either is fine for checks that
                // only gate on the text being short and well-formed.
                if text.as_ref().is_some_and(|t| t.is_empty()) {
                    text = def_text;
                }
            }
            info.text = text;
            if fault.is_none() && d > MAX_MACRO_DEPTH {
                fault = Some(format!(
                    "defined through more than {MAX_MACRO_DEPTH} other macros"
                ));
            }
            if fault.is_none() && info.len > MAX_MACRO_LEN {
                fault = Some(format!("expands past {} KiB", MAX_MACRO_LEN / 1024));
            }
            if let Some(why) = fault {
                self.empty_macro(&name, &defs, &why);
                info = MacroInfo {
                    len: 0,
                    text: Some(String::new()),
                };
                d = 1;
            }
            depth.insert(name.clone(), d);
            self.macro_info.insert(name.clone(), info);
            for user in users.get(&name).cloned().unwrap_or_default() {
                if let Some(n) = pending.get_mut(&user) {
                    *n -= 1;
                    if *n == 0 {
                        ready.push(user);
                    }
                }
            }
        }

        let mut cyclic: Vec<String> = by_name
            .keys()
            .filter(|n| !self.macro_info.contains_key(*n))
            .cloned()
            .collect();
        cyclic.sort();
        for name in cyclic {
            let defs = by_name[&name].clone();
            self.empty_macro(&name, &defs, "refers to itself");
            self.macro_info.insert(
                name,
                MacroInfo {
                    len: 0,
                    text: Some(String::new()),
                },
            );
        }
    }

    fn empty_macro(&mut self, name: &str, defs: &[usize], why: &str) {
        for &d in defs {
            let value = self.macros[d].value.clone();
            self.patch(value, "{}".to_string());
        }
        let span = self.macros[defs[0]].span.clone();
        let source = self.input[span].to_string();
        self.report(
            &source,
            format!("@string {name:?} {why}; its uses read as empty"),
        );
    }

    /// Per entry: rename duplicate keys, give unknown macros their name as
    /// text, defuse unreadable dates and runaway values, and fix the LaTeX
    /// `biblatex` misreads.
    fn check_entries(&mut self) {
        let mut taken: HashSet<String> = HashSet::new();
        // Each macro use re-parses the macro's text: the total is bounded
        // like the input it came from.
        let mut budget = budget(self.input);

        for e in 0..self.entries.len() {
            // A key equal to an earlier key (or `ids` alias) fails the whole
            // parse with DuplicateKey. Parse it under a fresh key; the CSL
            // id keeps the one written.
            let key = self.entries[e].key.clone();
            if taken.contains(&key) {
                let mut n = 2;
                let fresh = loop {
                    let candidate = format!("{key}-citeme-{n}");
                    if !taken.contains(&candidate) {
                        break candidate;
                    }
                    n += 1;
                };
                let span = self.entries[e].key_span.clone();
                self.patch(span, fresh.clone());
                self.entries[e].parse_key = fresh;
            }
            taken.insert(self.entries[e].parse_key.clone());

            for f in 0..self.entries[e].fields.len() {
                self.check_field(e, f, &mut budget);
            }

            if let Some(ids) = self.field_text(e, "ids") {
                for alias in ids.split(',').map(str::trim).filter(|a| !a.is_empty()) {
                    taken.insert(alias.to_string());
                }
            }

            let has_url = self.entries[e].fields.iter().any(|f| f.key == "url");
            if !has_url {
                let url = ["howpublished", "note"].iter().find_map(|key| {
                    let field = self.entries[e]
                        .fields
                        .iter()
                        .rev()
                        .find(|f| f.key == *key)?;
                    field.parts.iter().find_map(|part| match part {
                        Part::Text(range) => find_url(&self.input[range.clone()]),
                        Part::Macro(..) => None,
                    })
                });
                self.entries[e].url = url;
            }
        }
    }

    fn check_field(&mut self, e: usize, f: usize, budget: &mut usize) {
        let field = &self.entries[e].fields[f];
        let key = field.key.clone();
        let verbatim = is_verbatim(&key);
        let parts = field.parts.clone();
        let is_key = matches!(key.as_str(), "crossref" | "xdata" | "ids");
        // Dates and keys are checked as written; a rewrite would change what
        // the parser reads behind the checks' back.
        let rewrite = !(verbatim || is_key || is_date_field(&key));

        let mut inner_braces = 0usize;
        let mut expansion = 0usize;
        let mut rewrites = Vec::new();
        let mut unknown = Vec::new();
        for part in &parts {
            match part {
                Part::Text(range) => {
                    let raw = &self.input[range.clone()];
                    if is_key && raw.contains('\\') {
                        // Which entry a key names depends on LaTeX the
                        // parser runs; a cycle through it can't be seen.
                        return self.drop_field(e, f, "not a key (LaTeX)");
                    }
                    let rewritten = if rewrite { rewrite_latex(raw) } else { None };
                    let text = rewritten.as_deref().unwrap_or(raw);
                    match check_content(text, verbatim) {
                        Ok(stats) => inner_braces += stats.inner_braces,
                        Err(why) => return self.drop_field(e, f, why),
                    }
                    if let Some(new) = rewritten {
                        rewrites.push((range.clone(), new));
                    }
                }
                Part::Macro(name, range) => match self.macro_info.get(name) {
                    Some(info) => expansion = expansion.saturating_add(info.len),
                    None if month_name(name).is_some() => {}
                    // BibTeX reads an undefined macro as empty; keep the
                    // name, which says more than nothing.
                    None => unknown.push((name.clone(), range.clone())),
                },
            }
        }

        if inner_braces > MAX_INNER_BRACES {
            return self.drop_field(e, f, "too many nested braces");
        }
        if expansion > *budget {
            return self.drop_field(e, f, "@string expansion budget exhausted");
        }
        *budget -= expansion;

        // A date or a key the checks can't read whole (a macro expanding
        // past what they inspect) is no date or key anyone wrote.
        if is_key || is_date_field(&key) {
            let why = match self.field_text_at(e, f) {
                Some(text) => date_hazard(&key, &text),
                None => Some("too long to check"),
            };
            if let Some(why) = why {
                return self.drop_field(e, f, why);
            }
        }

        let preview = self.input[self.entries[e].span.clone()].to_string();
        for (name, range) in unknown {
            self.report(
                &preview,
                format!("{key}: unknown @string {name:?}, kept as text"),
            );
            rewrites.push((range, format!("{{{name}}}")));
        }
        for (range, new) in rewrites {
            self.patch(range, new);
        }
    }

    /// The value `biblatex` keeps for `key` — the last one written — as
    /// plain text, macros expanded, when short enough.
    fn field_text(&self, e: usize, key: &str) -> Option<String> {
        self.field_text_at(e, self.field_index(e, key)?)
    }

    fn field_text_at(&self, e: usize, f: usize) -> Option<String> {
        let entry = &self.entries[e];
        if entry.dropped.contains(&f) {
            return None;
        }
        let field = &entry.fields[f];
        let mut text = String::new();
        for part in &field.parts {
            match part {
                Part::Text(range) => text.push_str(&plain_text(&self.input[range.clone()])),
                Part::Macro(name, _) => match self.macro_info.get(name) {
                    Some(info) => text.push_str(info.text.as_deref()?),
                    None => text.push_str(month_name(name).map_or(name.as_str(), |m| m)),
                },
            }
            if text.len() > 4096 {
                return None;
            }
        }
        Some(text)
    }

    /// Bound `crossref`/`xdata` resolution: no cycles, no inheritance tree
    /// past `MAX_INHERITANCE`, no more inherited bytes than the input
    /// could pay for. Then make sure every entry something inherits from
    /// has a date `biblatex` can read — resolution reads it, and an error
    /// there fails the whole file.
    fn check_inheritance(&mut self) {
        // `biblatex` looks targets up by key, then `ids` alias, in insertion
        // order (a later alias overwrites).
        let mut index: HashMap<String, usize> = HashMap::new();
        for (i, entry) in self.entries.iter().enumerate() {
            index.insert(entry.parse_key.clone(), i);
            if let Some(ids) = self.field_text(i, "ids") {
                for alias in ids.split(',').map(str::trim).filter(|a| !a.is_empty()) {
                    index.insert(alias.to_string(), i);
                }
            }
        }

        let n = self.entries.len();
        let mut targets: Vec<Vec<usize>> = vec![Vec::new(); n];
        for (i, entry_targets) in targets.iter_mut().enumerate() {
            if let Some(key) = self.field_text(i, "crossref") {
                entry_targets.extend(index.get(key.trim()).copied());
            }
            if let Some(keys) = self.field_text(i, "xdata") {
                for key in keys.split(',').map(str::trim) {
                    entry_targets.extend(index.get(key).copied());
                }
            }
        }
        if targets.iter().all(Vec::is_empty) {
            return;
        }

        // Bottom-up over the target graph; entries never reached are on a
        // cycle or inherit from one.
        let mut pending: Vec<usize> = targets.iter().map(Vec::len).collect();
        let mut inheritors: Vec<Vec<usize>> = vec![Vec::new(); n];
        for (i, ts) in targets.iter().enumerate() {
            for &t in ts {
                inheritors[t].push(i);
            }
        }
        let mut ready: Vec<usize> = (0..n).filter(|&i| pending[i] == 0).collect();
        let mut size = vec![0usize; n];
        let mut inherited = vec![0usize; n];
        let mut done = vec![false; n];
        let mut cut: Vec<(usize, &'static str)> = Vec::new();
        while let Some(i) = ready.pop() {
            done[i] = true;
            let mut s = 1usize;
            let mut bytes = 0usize;
            for &t in &targets[i] {
                s = s.saturating_add(size[t]);
                let span = &self.entries[t].span;
                bytes = bytes
                    .saturating_add(span.end - span.start)
                    .saturating_add(inherited[t]);
            }
            if s > MAX_INHERITANCE {
                cut.push((i, "inherits from too many entries"));
                s = 1;
                bytes = 0;
            }
            size[i] = s;
            inherited[i] = bytes;
            for &user in &inheritors[i] {
                pending[user] -= 1;
                if pending[user] == 0 {
                    ready.push(user);
                }
            }
        }
        for (i, is_done) in done.iter().enumerate() {
            if !is_done {
                cut.push((i, "refers back to itself"));
            }
        }

        let mut is_cut = vec![false; n];
        for &(i, _) in &cut {
            is_cut[i] = true;
        }
        let mut budget = budget(self.input);
        for (i, &bytes) in inherited.iter().enumerate() {
            if done[i] && !is_cut[i] {
                if bytes > budget {
                    cut.push((i, "inherits more text than the file holds"));
                    is_cut[i] = true;
                } else {
                    budget -= bytes;
                }
            }
        }

        cut.sort_unstable();
        cut.dedup_by_key(|(i, _)| *i);
        for (i, why) in cut {
            self.drop_links(i, why);
        }

        let mut parents: Vec<usize> = (0..n)
            .filter(|&i| !is_cut[i])
            .flat_map(|i| targets[i].clone())
            .collect();
        parents.sort_unstable();
        parents.dedup();
        for p in parents {
            self.check_parent_date(p);
        }
    }

    fn drop_links(&mut self, e: usize, why: &str) {
        let fields: Vec<usize> = self.entries[e]
            .fields
            .iter()
            .enumerate()
            .filter(|(_, f)| f.key == "crossref" || f.key == "xdata")
            .map(|(i, _)| i)
            .collect();
        for f in fields {
            self.drop_field(e, f, why);
        }
    }

    /// `biblatex` reads a crossref parent's `year`/`month`/`day` while
    /// resolving its children, and a `TypeError` there fails the parse of
    /// every entry. Drop the part that would fail.
    fn check_parent_date(&mut self, e: usize) {
        if self.field_index(e, "date").is_some() {
            return;
        }
        let Some(year) = self.field_text(e, "year") else {
            return;
        };
        let month = self.field_text(e, "month");
        let day = self.field_text(e, "day");
        let failing = if year_fails(&year) {
            "year"
        } else if month
            .as_deref()
            .is_some_and(|m| month_fails(m, day.is_some()))
        {
            "month"
        } else if month.is_some() && day.as_deref().is_some_and(day_fails) {
            "day"
        } else {
            return;
        };
        if let Some(f) = self.field_index(e, failing) {
            self.drop_field(e, f, "not a date entries inheriting from this one can read");
        }
    }

    /// The field `biblatex` keeps for `key`: a repeated key overwrites.
    fn field_index(&self, e: usize, key: &str) -> Option<usize> {
        let entry = &self.entries[e];
        entry
            .fields
            .iter()
            .rposition(|f| f.key == key)
            .filter(|i| !entry.dropped.contains(i))
    }

    /// Parse the patched source. A failure the pre-scan didn't predict is
    /// defused at its span and the parse retried; past a few retries each
    /// entry is parsed on its own, so a further surprise costs one entry,
    /// not the file.
    fn parse(mut self) -> Guarded {
        let mut links_dropped = false;
        for _ in 0..MAX_RETRIES {
            let (source, map) = self.build();
            let error = match Bibliography::parse(&source) {
                Ok(bibliography) => return self.finish(bibliography.into_iter().collect()),
                Err(error) => error,
            };
            if !self.defuse(&error, map.original(error.span.start), &mut links_dropped) {
                break;
            }
        }
        let parsed = self.parse_each();
        self.finish(parsed)
    }

    /// Defuse what stopped a parse at `at`; false if nothing there can be.
    fn defuse(&mut self, error: &ParseError, at: usize, links_dropped: &mut bool) -> bool {
        let Some(e) = self.entry_at(at) else {
            let Some(name) = self.macro_at(at) else {
                return false;
            };
            let defs: Vec<usize> = (0..self.macros.len())
                .filter(|&d| self.macros[d].name == name)
                .collect();
            self.empty_macro(&name, &defs, &error.kind.to_string());
            return true;
        };
        match &error.kind {
            ParseErrorKind::DuplicateKey(_) => {
                let key = format!("{}-citeme-dup-{e}", self.entries[e].key);
                let span = self.entries[e].key_span.clone();
                self.patch(span, key.clone());
                self.entries[e].parse_key = key;
            }
            ParseErrorKind::ResolutionError(_) if !*links_dropped => {
                // The span may point anywhere in the inheritance chain; stop
                // resolving inheritance rather than guess.
                *links_dropped = true;
                for i in 0..self.entries.len() {
                    self.drop_links(i, "inheritance could not be resolved");
                }
            }
            kind => match self.field_at(e, at) {
                Some(f) => self.drop_field(e, f, &kind.to_string()),
                None => self.remove_entry(e, &kind.to_string()),
            },
        }
        true
    }

    fn remove_entry(&mut self, e: usize, why: &str) {
        let span = self.entries[e].span.clone();
        let preview = self.input[span.clone()].to_string();
        self.report(&preview, format!("biblatex parse error: {why}"));
        self.entries[e].removed = true;
        self.patch(span, String::new());
    }

    /// Each entry parsed alone, after the `@string` definitions it uses.
    /// Inheritance between entries is lost; the entries are not.
    fn parse_each(&mut self) -> Vec<biblatex::Entry> {
        let mut by_name: HashMap<String, Vec<usize>> = HashMap::new();
        for (i, def) in self.macros.iter().enumerate() {
            by_name.entry(def.name.clone()).or_default().push(i);
        }
        let mut budget = budget(self.input);
        let mut parsed = Vec::new();
        for e in 0..self.entries.len() {
            if self.entries[e].removed {
                continue;
            }
            let mut names: Vec<String> = self.entries[e]
                .fields
                .iter()
                .flat_map(|f| f.parts.iter())
                .filter_map(|part| match part {
                    Part::Macro(name, _) => Some(name.clone()),
                    Part::Text(_) => None,
                })
                .collect();
            let mut seen = HashSet::new();
            let mut defs = Vec::new();
            while let Some(name) = names.pop() {
                if !seen.insert(name.clone()) {
                    continue;
                }
                for &d in by_name.get(&name).map_or(&[][..], Vec::as_slice) {
                    defs.push(d);
                    for part in &self.macros[d].parts {
                        if let Part::Macro(used, _) = part {
                            names.push(used.clone());
                        }
                    }
                }
            }
            defs.sort_unstable();
            defs.dedup();
            let mut source = String::new();
            for d in defs {
                source.push_str("@string{");
                source.push_str(&self.patched(self.macros[d].span.clone()));
                source.push_str("}\n");
            }
            // A raw entry's span stops before its closing brace.
            source.push_str(&self.patched(self.entries[e].span.clone()));
            source.push('}');
            if source.len() > budget {
                self.remove_entry(e, "too large to read on its own");
                continue;
            }
            budget -= source.len();
            match Bibliography::parse(&source) {
                Ok(bibliography) => parsed.extend(bibliography),
                Err(error) => self.remove_entry(e, &error.kind.to_string()),
            }
        }
        parsed
    }

    /// `range` of the input with the patches inside it applied.
    fn patched(&self, range: Range<usize>) -> String {
        let mut out = String::new();
        let mut last = range.start;
        for (&start, (end, with)) in self.patches.range(range.start..range.end) {
            if start < last || *end > range.end {
                continue;
            }
            out.push_str(&self.input[last..start]);
            out.push_str(with);
            last = *end;
        }
        out.push_str(&self.input[last..range.end]);
        out
    }

    fn entry_at(&self, at: usize) -> Option<usize> {
        self.entries
            .iter()
            .position(|e| !e.removed && e.span.start <= at && at < e.span.end)
    }

    fn macro_at(&self, at: usize) -> Option<String> {
        self.macros
            .iter()
            .find(|m| m.span.start <= at && at < m.span.end)
            .map(|m| m.name.clone())
    }

    fn field_at(&self, e: usize, at: usize) -> Option<usize> {
        let entry = &self.entries[e];
        entry
            .fields
            .iter()
            .enumerate()
            .find(|(i, f)| !entry.dropped.contains(i) && f.span.start <= at && at < f.span.end)
            .map(|(i, _)| i)
    }

    fn build(&self) -> (String, OffsetMap) {
        let mut source = String::with_capacity(self.input.len());
        let mut map = OffsetMap::default();
        let mut last = 0;
        for (&start, (end, with)) in &self.patches {
            if start < last {
                // `patch` never overlaps two; never slice backwards anyway.
                continue;
            }
            source.push_str(&self.input[last..start]);
            map.segments
                .push((source.len(), start, with.len(), end - start));
            source.push_str(with);
            last = *end;
        }
        source.push_str(&self.input[last..]);
        (source, map)
    }

    fn finish(self, parsed: Vec<biblatex::Entry>) -> Guarded {
        let input = self.input;
        let notes = self
            .entries
            .into_iter()
            .filter(|e| !e.removed)
            .map(|e| {
                let notes = EntryNotes {
                    key: e.key,
                    url: e.url,
                    preview: input[e.span].trim().chars().take(80).collect(),
                };
                (e.parse_key, notes)
            })
            .collect();
        Guarded {
            parsed,
            errors: self.errors,
            notes,
            scanned: self.scanned,
        }
    }
}

/// Maps offsets in the patched source back to the input.
#[derive(Default)]
struct OffsetMap {
    /// (patched start, input start, patched length, input length).
    segments: Vec<(usize, usize, usize, usize)>,
}

impl OffsetMap {
    fn original(&self, at: usize) -> usize {
        let i = self
            .segments
            .partition_point(|&(patched, ..)| patched <= at);
        if i == 0 {
            return at;
        }
        let (patched, input, patched_len, input_len) = self.segments[i - 1];
        if at < patched + patched_len {
            input
        } else {
            input + input_len + (at - patched - patched_len)
        }
    }
}

/// What macro expansion, or inheritance, may add to the parse in all: in
/// proportion to the input, as a file legitimately using either stays.
fn budget(input: &str) -> usize {
    input.len().saturating_mul(8).saturating_add(4 << 20)
}

/// Where each chunk starts when a file has to be read piece by piece: the
/// start, and every `@` that opens a line. Entries sharing a line stay in
/// one chunk; a broken entry costs at most its own lines.
fn chunk_starts(input: &str) -> Vec<usize> {
    let mut starts = vec![0];
    let mut line_start = true;
    for (i, c) in input.char_indices() {
        if c == '\n' {
            line_start = true;
        } else if c == '@' && line_start {
            if i > 0 {
                starts.push(i);
            }
            line_start = false;
        } else if !c.is_whitespace() {
            line_start = false;
        }
    }
    starts
}

/// The lowercased `@type` a chunk opens with, if it opens with one.
fn chunk_kind(chunk: &str) -> Option<String> {
    let rest = chunk.trim_start().strip_prefix('@')?;
    let end = rest
        .find(|c: char| !c.is_alphanumeric())
        .unwrap_or(rest.len());
    Some(rest[..end].to_ascii_lowercase())
}

/// The fields `biblatex` reads verbatim: no commands, no math.
fn is_verbatim(field: &str) -> bool {
    matches!(
        field,
        "file" | "doi" | "uri" | "eprint" | "verba" | "verbb" | "verbc" | "pdf" | "url" | "urlraw"
    )
}

/// What `biblatex` resolves an undefined macro to when it names a month.
fn month_name(name: &str) -> Option<&'static str> {
    Some(match name.to_lowercase().as_str() {
        "jan" => "January",
        "feb" => "February",
        "mar" => "March",
        "apr" => "April",
        "may" => "May",
        "jun" => "June",
        "jul" => "July",
        "aug" => "August",
        "sep" => "September",
        "oct" => "October",
        "nov" => "November",
        "dec" => "December",
        _ => return None,
    })
}

/// A value as `biblatex`'s date and key readers see it, as far as signs,
/// digits and spaces go: escapes read as their character, `\-` (a
/// discretionary hyphen) as nothing, unescaped braces dropped, a run of
/// spaces as one, `--` and `---` as dashes. Other commands stay as written —
/// none of them prints a sign, a digit or a space.
fn plain_text(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    let mut chars = raw.chars().peekable();
    let mut space = false;
    while let Some(c) = chars.next() {
        match c {
            '\\' => match chars.peek() {
                Some('-') => {
                    chars.next();
                }
                Some(&n @ ('{' | '}' | '\\' | '#' | '&' | '%' | '$' | '_' | ':')) => {
                    chars.next();
                    out.push(n);
                    space = false;
                }
                _ => {
                    out.push(c);
                    space = false;
                }
            },
            '{' | '}' => {}
            c if c.is_whitespace() => {
                if !space {
                    out.push(' ');
                }
                space = true;
            }
            '-' => {
                let mut run = 1;
                while chars.peek() == Some(&'-') {
                    chars.next();
                    run += 1;
                }
                for _ in 0..run / 3 {
                    out.push('—');
                }
                match run % 3 {
                    1 => out.push('-'),
                    2 => out.push('–'),
                    _ => {}
                }
                space = false;
            }
            c => {
                out.push(c);
                space = false;
            }
        }
    }
    out
}

/// A field `biblatex` reads as part of a date: `date`, `year`, `month`,
/// `day`, each also with an `url`, `event` or `orig` prefix.
fn is_date_field(field: &str) -> bool {
    matches!(date_part(field), "date" | "year" | "month" | "day")
}

fn date_part(field: &str) -> &str {
    ["url", "event", "orig"]
        .iter()
        .find_map(|prefix| field.strip_prefix(prefix))
        .unwrap_or(field)
}

/// Why `biblatex`'s date parser would panic (or, on a numeric month, read
/// garbage) on this value of a date field, if it would.
fn date_hazard(field: &str, text: &str) -> Option<&'static str> {
    match date_part(field) {
        "date" if has_foreign_digit(text) => Some("not a date (non-ASCII digits)"),
        "date" if bad_unspecified_date(text) => Some("not a date"),
        "year" if signed_year_with_space(text) => Some("not a year"),
        "month" if month_hazard(text) => Some("not a month"),
        _ => None,
    }
}

/// `biblatex` scans date digits with `char::is_numeric` but parses them as
/// ASCII: "²²" or "٢٠" passes the scan and panics the parse.
fn has_foreign_digit(text: &str) -> bool {
    text.chars().any(|c| {
        (c.is_numeric() && !c.is_ascii_digit())
            || c.to_uppercase()
                .any(|u| u.is_numeric() && !u.is_ascii_digit())
    })
}

/// An EDTF date with unspecified digits ("19XX") goes down a branch that
/// subtracts the digit count from 4 and a month from 1 unchecked: more than
/// four leading digits, or month "00", overflows.
fn bad_unspecified_date(text: &str) -> bool {
    let upper = text.to_uppercase();
    if !upper.contains('X') {
        return false;
    }
    let s = upper.trim_start();
    let digits = s.bytes().take_while(u8::is_ascii_digit).count();
    if digits > 4 {
        return true;
    }
    if digits < 4 {
        return false;
    }
    let rest = s[digits..].trim_start();
    let Some(rest) = rest.strip_prefix('-') else {
        return false;
    };
    let rest = rest.trim_start_matches('-').trim_start();
    let month = rest.bytes().take_while(u8::is_ascii_digit).count();
    month == 2 && &rest[..2] == "00"
}

/// `year = {- 2020}`: the year parser takes sign, spaces and digits in one
/// slice and `parse::<i32>().unwrap()`s it.
fn signed_year_with_space(text: &str) -> bool {
    let s = text.trim_start();
    let Some(rest) = s.strip_prefix(['+', '-']) else {
        return false;
    };
    let after = rest.trim_start();
    after.len() < rest.len() && after.starts_with(|c: char| c.is_ascii_digit())
}

/// The ways a `month` value breaks the date parser: a day read from it
/// past 255 (`may 300`, `5 300` — `u8` `unwrap`), a non-ASCII digit, or a
/// numeric month outside 1–12 (biblatex 0.12 reads `0` as `0 - 1`).
fn month_hazard(text: &str) -> bool {
    if has_foreign_digit(text) {
        return true;
    }
    let s = text.trim_start();
    let lead = s.bytes().take_while(u8::is_ascii_digit).count();
    if (1..=2).contains(&lead) && !(1..=12).contains(&s[..lead].parse::<u32>().unwrap_or(0)) {
        return true;
    }
    let word = s
        .find(|c: char| !(c.is_ascii_alphanumeric() || c.is_numeric()))
        .unwrap_or(s.len());
    let rest = &s[word..];
    let day = rest.trim_start_matches(|c: char| c.is_whitespace() || c == '-');
    if day.len() == rest.len() {
        return false;
    }
    let digits = day.bytes().take_while(u8::is_ascii_digit).count();
    digits > 0 && day[..digits].parse::<u32>().map_or(true, |d| d > 255)
}

/// Whether `biblatex`'s year parser returns an error for this text.
fn year_fails(text: &str) -> bool {
    let s = text.trim_start();
    let signed = s.starts_with(['+', '-']);
    let s = if signed { s[1..].trim_start() } else { s };
    let digits = s.bytes().take_while(u8::is_ascii_digit).count();
    if digits == 0 || digits > 4 {
        return true;
    }
    let year: u32 = s[..digits].parse().unwrap_or(0);
    let rest = s[digits..].trim_start();
    let era = if let Some(after) = rest.strip_prefix("AD").or_else(|| rest.strip_prefix("CE")) {
        Some(after)
    } else {
        rest.strip_prefix("BC")
            .map(|after| after.strip_prefix('E').unwrap_or(after))
    };
    match era {
        Some(after) => after.starts_with(|c: char| c.is_alphanumeric()) || signed || year == 0,
        None => false,
    }
}

/// Whether reading a day out of this `month` value fails (only attempted
/// when there is no `day` field).
fn month_fails(text: &str, has_day: bool) -> bool {
    if has_day {
        return false;
    }
    let s = text.trim_start();
    let word = s
        .find(|c: char| !(c.is_ascii_alphanumeric() || c.is_numeric()))
        .unwrap_or(s.len());
    let rest = &s[word..];
    let day = rest.trim_start_matches(|c: char| c.is_whitespace() || c == '-' || c == '\u{a0}');
    if day.len() == rest.len() {
        return false;
    }
    let digits = day.bytes().take_while(u8::is_ascii_digit).count();
    digits == 0 || !(1..=31).contains(&day[..digits].parse::<u32>().unwrap_or(0))
}

fn day_fails(text: &str) -> bool {
    !text
        .trim()
        .parse::<u8>()
        .is_ok_and(|d| (1..=31).contains(&d))
}

struct ContentStats {
    inner_braces: usize,
}

/// Run `biblatex`'s content parser (`resolve::ContentParser`) dry: does
/// this value error, how deep do its command arguments recurse, and how
/// many chunk boundaries does it open inside braces? Iterative — arguments
/// go on a worklist instead of the call stack the real parser uses.
fn check_content(text: &str, verbatim: bool) -> Result<ContentStats, &'static str> {
    let mut stats = ContentStats { inner_braces: 0 };
    let mut work = vec![(0..text.len(), 0usize, verbatim)];
    while let Some((range, level, verb)) = work.pop() {
        if level > MAX_COMMAND_DEPTH {
            return Err("LaTeX commands nested too deeply");
        }
        let s = &text[range.clone()];
        let mut depth = 0usize;
        let mut i = 0;
        while let Some(c) = s[i..].chars().next() {
            match c {
                '\\' => {
                    i += 1;
                    match s[i..].chars().next() {
                        Some(n) if n != '^' && n != '~' && is_escapable(n, verb) => {
                            i += n.len_utf8()
                        }
                        _ if verb => {}
                        Some(n) if !n.is_whitespace() && !n.is_control() => {
                            let name_start = i;
                            i += n.len_utf8();
                            if !is_single_char_func(n, false) {
                                i += s[i..]
                                    .find(|m: char| !is_id_continue(m))
                                    .unwrap_or(s.len() - i);
                            }
                            let one_char = s[name_start..i].chars().count() == 1;
                            let after = s[i..].trim_start_matches(char::is_whitespace);
                            let ws = after.len() < s.len() - i;
                            i = s.len() - after.len();
                            let next = after.chars().next();
                            if next != Some('{')
                                && one_char
                                && n != '-'
                                && is_single_char_func(n, ws)
                            {
                                i += next.map_or(0, char::len_utf8);
                            } else if !ws && next == Some('{') {
                                i += 1;
                                let arg = i;
                                let mut open = 1usize;
                                loop {
                                    let Some(at) = s[i..].find(['{', '}']) else {
                                        return Err("unclosed LaTeX argument");
                                    };
                                    i += at + 1;
                                    if s.as_bytes()[i - 1] == b'{' {
                                        open += 1;
                                    } else {
                                        open -= 1;
                                        if open == 0 {
                                            break;
                                        }
                                    }
                                }
                                let start = range.start + arg;
                                work.push((start..range.start + i - 1, level + 1, false));
                            }
                        }
                        Some(_) => {}
                        None => return Err("ends in a backslash"),
                    }
                }
                '$' if !verb => {
                    i += 1;
                    match s[i..].find('$') {
                        Some(at) => i += at + 1,
                        None => return Err("unclosed math"),
                    }
                }
                '{' => {
                    if depth >= 1 && level == 0 {
                        stats.inner_braces += 1;
                    }
                    depth += 1;
                    i += 1;
                }
                '}' => {
                    if depth == 0 {
                        return Err("unbalanced braces");
                    }
                    depth -= 1;
                    if depth >= 1 && level == 0 {
                        stats.inner_braces += 1;
                    }
                    i += 1;
                }
                c => i += c.len_utf8(),
            }
        }
    }
    Ok(stats)
}

fn is_escapable(c: char, verbatim: bool) -> bool {
    match c {
        '{' | '}' | '\\' | ':' => true,
        '~' | '^' | '#' | '&' | '%' | '$' | '_' => !verbatim,
        _ => false,
    }
}

fn is_single_char_func(c: char, ws: bool) -> bool {
    matches!(
        c,
        '"' | '´' | '`' | '\'' | '^' | '~' | '=' | '.' | '\\' | '-'
    ) || (ws && matches!(c, 'b' | 'c' | 'd' | 'H' | 'k' | 'r' | 'u' | 'v'))
}

fn is_id_continue(c: char) -> bool {
    !matches!(
        c,
        '@' | '{' | '}' | '"' | '#' | '\'' | '(' | ')' | ',' | '=' | '%' | '\\' | '~'
    ) && !c.is_control()
        && !c.is_whitespace()
}

/// Formatting commands whose argument is plain text to us. `biblatex`
/// keeps unknown commands verbatim, so `\emph{Drosophila}` reached the CSL
/// title as written.
const TEXT_COMMANDS: &[&str] = &[
    "emph",
    "textit",
    "textbf",
    "textsc",
    "textsl",
    "textsf",
    "texttt",
    "textrm",
    "textup",
    "textmd",
    "textnormal",
    "textsubscript",
    "textsuperscript",
    "mbox",
    "url",
    "NoCaseChange",
];

/// Font switches (`{\it Old}`), which apply to the rest of their group.
const DECLARATIONS: &[&str] = &[
    "it",
    "em",
    "bf",
    "sc",
    "sl",
    "sf",
    "tt",
    "rm",
    "up",
    "md",
    "itshape",
    "bfseries",
    "scshape",
    "slshape",
    "sffamily",
    "ttfamily",
    "rmfamily",
    "upshape",
    "mdseries",
    "normalfont",
];

/// Commands that print a letter and take no argument. `biblatex` takes a
/// group right after them as one anyway, and drops it (`\ss{x}`) — or, for
/// `\o`, turns it into a combining stroke: `J\o{}rgensen` → "J̸rgensen".
const LETTERS: &[&str] = &[
    "o", "O", "aa", "AA", "ae", "AE", "oe", "OE", "l", "L", "ss", "SS", "i", "dh", "DH", "dj",
    "DJ", "ng", "NG", "th", "TH",
];

/// Rewrite the LaTeX `biblatex` mishandles in a non-verbatim value, or
/// `None` if there is none: text commands lose their name (`\emph{x}` →
/// `{x}`), font switches go with the space after them, and letter commands
/// are fenced off from a following group (`\o{}` → `{\o}{}`).
fn rewrite_latex(raw: &str) -> Option<String> {
    if !raw.contains('\\') {
        return None;
    }
    let mut out = String::with_capacity(raw.len());
    let mut changed = false;
    let mut i = 0;
    while let Some(c) = raw[i..].chars().next() {
        if c != '\\' {
            out.push(c);
            i += c.len_utf8();
            continue;
        }
        let Some(n) = raw[i + 1..].chars().next() else {
            out.push(c);
            break;
        };
        if !n.is_ascii_alphabetic() {
            // An escape or a one-character command: copy both.
            out.push(c);
            out.push(n);
            i += 1 + n.len_utf8();
            continue;
        }
        let name_end = raw[i + 1..]
            .find(|m: char| !is_id_continue(m))
            .map_or(raw.len(), |at| i + 1 + at);
        let name = &raw[i + 1..name_end];
        let next = raw[name_end..].chars().next();
        if TEXT_COMMANDS.contains(&name) {
            changed = true;
        } else if DECLARATIONS.contains(&name) {
            changed = true;
            i = raw.len() - raw[name_end..].trim_start().len();
            continue;
        } else if LETTERS.contains(&name) && next == Some('{') {
            changed = true;
            out.push('{');
            out.push_str(&raw[i..name_end]);
            out.push('}');
        } else {
            out.push_str(&raw[i..name_end]);
        }
        i = name_end;
    }
    changed.then_some(out)
}

/// The first `\url{…}` (or `\href{…}`) target in a value.
fn find_url(raw: &str) -> Option<String> {
    let start = ["\\url{", "\\href{"]
        .iter()
        .filter_map(|cmd| raw.find(cmd).map(|at| at + cmd.len()))
        .min()?;
    let mut open = 1usize;
    for (at, c) in raw[start..].char_indices() {
        match c {
            '{' => open += 1,
            '}' => {
                open -= 1;
                if open == 0 {
                    let url = raw[start..start + at].trim();
                    return (!url.is_empty()).then(|| url.to_string());
                }
            }
            _ => {}
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rewrite_latex_strips_formatting_and_fences_letters() {
        assert_eq!(
            rewrite_latex(r"The \emph{Drosophila} genome").as_deref(),
            Some("The {Drosophila} genome")
        );
        assert_eq!(
            rewrite_latex(r"{\it Old} style").as_deref(),
            Some("{Old} style")
        );
        assert_eq!(
            rewrite_latex(r"CO\textsubscript{2}").as_deref(),
            Some("CO{2}")
        );
        assert_eq!(
            rewrite_latex(r"J\o{}rgensen").as_deref(),
            Some(r"J{\o}{}rgensen")
        );
        assert_eq!(rewrite_latex(r"\'{e}t\'{e} \{x\}"), None);
        assert_eq!(rewrite_latex(r"\oe uvre \o rn"), None);
        assert_eq!(rewrite_latex("plain"), None);
    }

    #[test]
    fn check_content_mirrors_the_parser_errors() {
        assert!(check_content(r"\emph{x}", false).is_ok());
        assert!(check_content("a $x$ b", false).is_ok());
        assert!(check_content("a $x b", false).is_err());
        assert!(check_content("a $x b", true).is_ok());
        assert!(check_content(r"\emph{a\}b}", false).is_err());
        assert!(check_content("a}b", false).is_err());
        let nested = format!("{}x{}", r"\emph{".repeat(40), "}".repeat(40));
        assert!(check_content(&nested, false).is_err());
        let flat = format!("{{{}}}", "{a}".repeat(10));
        assert_eq!(check_content(&flat, false).unwrap().inner_braces, 20);
    }

    #[test]
    fn date_hazards_cover_each_panic() {
        assert!(date_hazard("date", "²²").is_some());
        assert!(date_hazard("urldate", "2020-٢").is_some());
        assert!(date_hazard("date", "12345X").is_some());
        assert!(date_hazard("date", "2020-00-XX").is_some());
        assert!(date_hazard("date", "19XX").is_none());
        assert!(date_hazard("date", "2020-05-03/2021").is_none());
        assert!(date_hazard("year", "- 2020").is_some());
        assert!(date_hazard("year", "-2020").is_none());
        assert!(date_hazard("year", "in press").is_none());
        assert!(date_hazard("month", "may 300").is_some());
        assert!(date_hazard("eventmonth", "5 300").is_some());
        assert!(date_hazard("month", "0").is_some());
        assert!(date_hazard("month", "13").is_some());
        assert!(date_hazard("month", "may 3").is_none());
        assert!(date_hazard("month", "5").is_none());
    }

    #[test]
    fn year_fails_like_biblatex() {
        assert!(!year_fails("2020"));
        assert!(!year_fails("2020a"));
        assert!(!year_fails("-44"));
        assert!(!year_fails("44 BC"));
        assert!(year_fails("in press"));
        assert!(year_fails("12345"));
        assert!(year_fails("0 AD"));
        assert!(year_fails("44 BCX"));
    }

    #[test]
    fn parse_each_reads_entries_alone_with_their_macros() {
        use biblatex::ChunksExt;
        let input = "@string{j = {Journal}}\n@article{a, title = {T}, journal = j # { X}}\n@book{a, title = {\\o{}l}}";
        let mut guard = Guard::read(input);
        guard.check_macros();
        guard.check_entries();
        let parsed = guard.parse_each();
        assert_eq!(parsed.len(), 2, "{:?}", guard.errors);
        let field = |i: usize, key: &str| parsed[i].get(key).map(|c| c.format_verbatim());
        assert_eq!(field(0, "journal").as_deref(), Some("Journal X"));
        assert_eq!(field(1, "title").as_deref(), Some("øl"));
    }

    #[test]
    fn offset_map_round_trips_through_patches() {
        let map = OffsetMap {
            segments: vec![(5, 5, 2, 10), (12, 20, 0, 3)],
        };
        assert_eq!(map.original(3), 3);
        assert_eq!(map.original(6), 5);
        assert_eq!(map.original(7), 15);
        assert_eq!(map.original(12), 23);
    }
}
