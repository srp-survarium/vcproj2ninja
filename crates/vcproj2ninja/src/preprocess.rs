//! A small, dependency-only C/C++ preprocessor.
//!
//! Its single job is to discover, for a translation unit, the set of header
//! files it transitively `#include`s so the generator can list them as ninja
//! implicit inputs — making header edits force a recompile. As a side effect it
//! also collects `#pragma comment(lib, "...")` directives (not yet wired into
//! the linker; just surfaced for now).
//!
//! # Why a real preprocessor (and not just "grep #include")
//!
//! `#include`s live behind `#if`/`#ifdef`, and `#if 0 ... #endif` blocks
//! routinely wrap dead includes. We evaluate the conditionals so those are
//! pruned. But we deliberately bias towards **over-approximation**: when a
//! condition can't be decided from the macros we know (the `/D` defines plus a
//! tiny builtin set), we traverse the branch anyway. The reasoning:
//!
//!  - Missing a real dependency is a *correctness* bug (stale object files).
//!  - Listing a header that wouldn't actually be compiled in only costs an
//!    occasional spurious rebuild — and only if that header even exists on disk
//!    within the project's include dirs.
//!
//! So unknown identifiers in a `#if` evaluate to "unknown" (traverse), never to
//! C's usual `0` (which could wrongly prune a branch whose controlling macro we
//! simply didn't observe). For the same reason we do **not** track in-file
//! `#define`/`#undef`: a define leaking across the multiple branches we walk
//! could turn an "unknown" into a wrong "decided" and prune a live branch.
//!
//! Only headers reachable through the project's own `/I` directories (or quote
//! includes relative to the including file) are resolved; system headers from
//! `%INCLUDE%` never resolve here and thus never become dependencies — exactly
//! what we want, since they don't change.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

/// Caches file contents so a header shared by many translation units / groups is
/// read from disk only once per generator run.
#[derive(Default)]
pub struct FileCache {
    files: HashMap<PathBuf, Option<String>>,
}

impl FileCache {
    fn read(&mut self, path: &Path) -> Option<String> {
        self.files
            .entry(path.to_path_buf())
            .or_insert_with(|| std::fs::read_to_string(path).ok())
            .clone()
    }
}

/// Outcome of scanning one or more translation units.
pub struct ScanResult {
    /// Transitively-included headers that exist on disk (normalized, absolute).
    pub headers: Vec<PathBuf>,
    /// Library names from `#pragma comment(lib, "...")` in active regions.
    pub pragma_libs: Vec<String>,
    /// Unrecognized `#`-directives encountered, deduplicated by keyword (one
    /// sample per distinct keyword). The caller surfaces these as warnings.
    pub unknown_directives: Vec<UnknownDirective>,
}

/// Scan `sources` (root .cpp/.c files of one cl group) for their transitive
/// header dependencies and `#pragma comment(lib)` directives.
///
/// `include_dirs` are the resolved `/I` search directories. `defines` are the
/// raw `/D` strings (`NAME` or `NAME=VALUE`). `cache` is shared to avoid
/// re-reading shared headers.
pub fn scan_translation_units(
    sources: &[PathBuf],
    include_dirs: &[PathBuf],
    defines: &[String],
    cache: &mut FileCache,
) -> ScanResult {
    let mut macros: HashMap<String, String> = HashMap::new();

    // Builtins that are accurate for the target (VS2008 / MSVC9, Windows). They
    // only ever make pruning *more* precise; everything unknown is traversed.
    macros.insert("_WIN32".to_string(), "1".to_string());
    macros.insert("_MSC_VER".to_string(), "1500".to_string());

    for define in defines {
        let define = define.trim();
        if define.is_empty() {
            continue;
        }
        let (name, value) = match define.split_once('=') {
            // A bare `/D FOO` defines FOO as 1 in MSVC.
            None => (define.to_string(), "1".to_string()),
            Some((name, value)) => (name.trim().to_string(), value.trim().to_string()),
        };
        macros.insert(name, value);
    }

    let mut scanner = Scanner {
        include_dirs,
        macros,
        cache,
        visited: HashSet::new(),
        headers: Vec::new(),
        header_seen: HashSet::new(),
        pragma_libs: Vec::new(),
        pragma_seen: HashSet::new(),
        unknown_directives: Vec::new(),
        unknown_seen: HashSet::new(),
    };

    for source in sources {
        scanner.process_file(source, true);
    }

    ScanResult {
        headers: scanner.headers,
        pragma_libs: scanner.pragma_libs,
        unknown_directives: scanner.unknown_directives,
    }
}

struct Scanner<'a> {
    include_dirs: &'a [PathBuf],
    macros: HashMap<String, String>,
    cache: &'a mut FileCache,
    /// Files already walked — dedupes work and models include guards /
    /// `#pragma once` (re-inclusion is simply skipped).
    visited: HashSet<PathBuf>,
    headers: Vec<PathBuf>,
    header_seen: HashSet<PathBuf>,
    pragma_libs: Vec<String>,
    pragma_seen: HashSet<String>,
    unknown_directives: Vec<UnknownDirective>,
    unknown_seen: HashSet<String>,
}

impl Scanner<'_> {
    fn process_file(&mut self, path: &Path, is_root: bool) {
        let canon = normalize(path);

        if !self.visited.insert(canon.clone()) {
            return;
        }

        let Some(content) = self.cache.read(&canon) else {
            // Unreadable (e.g. an angle include we mistakenly resolved, or a
            // permissions issue). It is still recorded as visited so we don't
            // keep retrying, but it must NOT become a dependency.
            self.visited.remove(&canon);
            return;
        };

        // A header (not the root TU) that we could read is a real dependency.
        if !is_root && self.header_seen.insert(canon.clone()) {
            self.headers.push(canon.clone());
        }

        let dir = canon.parent().unwrap_or(Path::new(""));
        let lines = preprocess_text(&content);

        // Conditional inclusion stack. We only act on `#include`/`#pragma` when
        // every enclosing branch is (possibly) active.
        let mut stack: Vec<Frame> = Vec::new();

        for line in &lines {
            let Some(directive) = parse_directive(line) else {
                continue;
            };

            match directive {
                Directive::If(expr) => {
                    let parent = active(&stack);
                    let frame = if !parent {
                        Frame::dead(parent)
                    } else {
                        Frame::from_tri(parent, self.eval_if(&expr))
                    };
                    stack.push(frame);
                }
                Directive::Ifdef(name) => {
                    let parent = active(&stack);
                    let tri = self.tri_defined(&name);
                    stack.push(Frame::from_tri(parent, tri));
                }
                Directive::Ifndef(name) => {
                    let parent = active(&stack);
                    let tri = self.tri_defined(&name).negate();
                    stack.push(Frame::from_tri(parent, tri));
                }
                Directive::Elif(expr) => {
                    if let Some(top) = stack.last_mut() {
                        if !top.parent_active || top.taken {
                            top.active = false;
                        } else {
                            match self.eval_if(&expr) {
                                Tri::True => {
                                    top.active = true;
                                    top.taken = true;
                                }
                                Tri::False => top.active = false,
                                Tri::Unknown => top.active = true,
                            }
                        }
                    }
                }
                Directive::Else => {
                    if let Some(top) = stack.last_mut() {
                        if !top.parent_active || top.taken {
                            top.active = false;
                        } else {
                            top.active = true;
                            top.taken = true;
                        }
                    }
                }
                Directive::Endif => {
                    stack.pop();
                }
                Directive::Include(spec) => {
                    if active(&stack) {
                        if let Some((kind, name)) = parse_include(&spec) {
                            if let Some(resolved) = self.resolve_include(dir, kind, &name) {
                                self.process_file(&resolved, false);
                            }
                        }
                    }
                }
                Directive::Pragma(rest) => {
                    if active(&stack) {
                        if let Some(lib) = parse_pragma_comment_lib(&rest) {
                            if self.pragma_seen.insert(lib.clone()) {
                                self.pragma_libs.push(lib);
                            }
                        }
                    }
                }
                Directive::Ignored => {}
                Directive::Unknown { keyword } => {
                    if self.unknown_seen.insert(keyword.clone()) {
                        self.unknown_directives.push(UnknownDirective {
                            keyword,
                            line: line.trim().to_string(),
                            file: canon.clone(),
                        });
                    }
                }
            }
        }
    }

    /// `defined NAME`: known macro → True; otherwise Unknown (open-world: the
    /// macro may come from a system header or define we don't track).
    fn tri_defined(&self, name: &str) -> Tri {
        if self.macros.contains_key(name) {
            Tri::True
        } else {
            Tri::Unknown
        }
    }

    fn eval_if(&self, expr: &str) -> Tri {
        match eval_expr(expr, &self.macros) {
            Some(0) => Tri::False,
            Some(_) => Tri::True,
            None => Tri::Unknown,
        }
    }

    fn resolve_include(&mut self, current_dir: &Path, kind: IncludeKind, name: &str) -> Option<PathBuf> {
        let name = name.replace('\\', "/");

        // Quote includes look next to the including file first; angle includes
        // search only the configured include directories.
        let mut candidates: Vec<PathBuf> = Vec::new();
        if matches!(kind, IncludeKind::Quote) {
            candidates.push(current_dir.join(&name));
        }
        for dir in self.include_dirs {
            candidates.push(dir.join(&name));
        }

        for candidate in candidates {
            let normalized = normalize(&candidate);
            if normalized.is_file() {
                return Some(normalized);
            }
        }
        None
    }
}

/// A frame of the conditional-inclusion stack.
struct Frame {
    /// Is *this* branch (possibly) active, given its own condition?
    active: bool,
    /// Has a branch in this if/elif/else chain been *definitely* taken? If so,
    /// later branches are dead. Only a proven-true condition sets this.
    taken: bool,
    /// Was the enclosing context active when this `#if` opened?
    parent_active: bool,
}

impl Frame {
    fn dead(parent_active: bool) -> Self {
        Frame {
            active: false,
            taken: false,
            parent_active,
        }
    }

    fn from_tri(parent_active: bool, tri: Tri) -> Self {
        if !parent_active {
            return Frame::dead(parent_active);
        }
        match tri {
            Tri::True => Frame {
                active: true,
                taken: true,
                parent_active,
            },
            Tri::False => Frame {
                active: false,
                taken: false,
                parent_active,
            },
            // Unknown: traverse the branch, but don't claim it as taken so the
            // following #elif/#else are walked too (over-approximation).
            Tri::Unknown => Frame {
                active: true,
                taken: false,
                parent_active,
            },
        }
    }
}

fn active(stack: &[Frame]) -> bool {
    stack.iter().all(|f| f.active)
}

#[derive(Clone, Copy)]
enum Tri {
    True,
    False,
    Unknown,
}

impl Tri {
    fn negate(self) -> Tri {
        match self {
            Tri::True => Tri::False,
            Tri::False => Tri::True,
            Tri::Unknown => Tri::Unknown,
        }
    }
}

#[derive(Clone, Copy)]
enum IncludeKind {
    Quote,
    Angle,
}

enum Directive {
    If(String),
    Ifdef(String),
    Ifndef(String),
    Elif(String),
    Else,
    Endif,
    Include(String),
    Pragma(String),
    /// A directive we knowingly skip (`#define`, `#undef`, the null directive).
    Ignored,
    /// A `#`-prefixed directive we don't recognize. Surfaced so we can see what
    /// the scanner is failing to follow (it might matter for dependencies).
    Unknown { keyword: String },
}

/// A `#`-prefixed line whose directive keyword the scanner doesn't handle.
pub struct UnknownDirective {
    pub keyword: String,
    /// The full directive line (trimmed), for context.
    pub line: String,
    /// The file it was found in.
    pub file: PathBuf,
}

/// Recognize a preprocessor directive.
///
/// A line that doesn't begin with `#` is not a directive → `None`. A line that
/// *does* begin with `#` is always classified: handled directives map to their
/// variant; `#define`/`#undef` (and the null `#`) map to [`Directive::Ignored`]
/// on purpose (see module docs); anything else becomes [`Directive::Unknown`] so
/// the caller can warn about it rather than silently dropping it.
fn parse_directive(line: &str) -> Option<Directive> {
    let rest = line.trim_start().strip_prefix('#')?;
    let rest = rest.trim_start();

    let end = rest
        .find(|c: char| !c.is_ascii_alphanumeric() && c != '_')
        .unwrap_or(rest.len());
    let (keyword, args) = rest.split_at(end);
    let args = args.trim();

    let first_ident = || args.split_whitespace().next().unwrap_or("").to_string();

    Some(match keyword {
        "if" => Directive::If(args.to_string()),
        "ifdef" => Directive::Ifdef(first_ident()),
        "ifndef" => Directive::Ifndef(first_ident()),
        "elif" => Directive::Elif(args.to_string()),
        "else" => Directive::Else,
        "endif" => Directive::Endif,
        "include" => Directive::Include(args.to_string()),
        "pragma" => Directive::Pragma(args.to_string()),
        // Deliberately not followed. `#define`/`#undef` are skipped by design
        // (see module docs). `#error`/`#warning`/`#line` are pure diagnostics
        // with no bearing on which files get included. `#include_next` *is* an
        // include, but a tree-wide sweep of the vostok sources showed every use
        // targets a standard-library / CRT header (stlport & boost wrapping
        // system headers), never a project header — so following it would only
        // reach untracked system headers. Revisit if a project ever chains
        // `#include_next` between same-named *project* headers.
        "define" | "undef" | "error" | "warning" | "line" | "include_next" => Directive::Ignored,
        // The null directive (`#` alone) and GNU line markers (`# 123 "file"`).
        keyword if keyword.is_empty() || keyword.bytes().all(|b| b.is_ascii_digit()) => {
            Directive::Ignored
        }
        keyword => Directive::Unknown {
            keyword: keyword.to_string(),
        },
    })
}

/// Parse the operand of an `#include`. Computed (macro) includes return `None`.
fn parse_include(spec: &str) -> Option<(IncludeKind, String)> {
    let spec = spec.trim();
    if let Some(rest) = spec.strip_prefix('"') {
        let end = rest.find('"')?;
        Some((IncludeKind::Quote, rest[..end].to_string()))
    } else if let Some(rest) = spec.strip_prefix('<') {
        let end = rest.find('>')?;
        Some((IncludeKind::Angle, rest[..end].to_string()))
    } else {
        None
    }
}

/// Extract the lib name from `comment(lib, "name.lib")`. Returns `None` for any
/// other `#pragma`.
fn parse_pragma_comment_lib(rest: &str) -> Option<String> {
    let after = rest.trim_start().strip_prefix("comment")?;
    let after = after.trim_start();
    let inner = after.strip_prefix('(')?;
    let lib_pos = inner.find("lib")?;
    let after_lib = &inner[lib_pos + 3..];
    let q1 = after_lib.find('"')?;
    let after_q1 = &after_lib[q1 + 1..];
    let q2 = after_q1.find('"')?;
    let name = after_q1[..q2].trim();
    if name.is_empty() {
        None
    } else {
        Some(name.to_string())
    }
}

/// Normalize a path lexically (collapse `.`/`..`), falling back to the input on
/// the rare paths that can't be normalized.
fn normalize(path: &Path) -> PathBuf {
    path.normalize_lexically().unwrap_or_else(|_| path.to_path_buf())
}

/// Phase 2+3 of translation: splice `\`-newline continuations and strip
/// comments, preserving newlines so the result still splits into logical lines.
fn preprocess_text(raw: &str) -> Vec<String> {
    // Normalize line endings, then splice backslash-newline continuations.
    let unified = raw.replace("\r\n", "\n").replace('\r', "\n");
    let spliced = unified.replace("\\\n", "");
    let stripped = strip_comments(&spliced);
    stripped.split('\n').map(|s| s.to_string()).collect()
}

#[derive(Clone, Copy)]
enum CommentState {
    Normal,
    Str,
    Chr,
    Line,
    Block,
}

/// Remove `//` and `/* */` comments while respecting string and char literals.
/// Comment bodies become spaces; newlines are preserved. String/char literals
/// are force-terminated at a newline so a stray quote can't swallow the rest of
/// the file.
fn strip_comments(s: &str) -> String {
    let chars: Vec<char> = s.chars().collect();
    let n = chars.len();
    let mut out = String::with_capacity(s.len());
    let mut state = CommentState::Normal;
    let mut i = 0;

    while i < n {
        let c = chars[i];
        match state {
            CommentState::Normal => {
                if c == '/' && i + 1 < n && chars[i + 1] == '/' {
                    state = CommentState::Line;
                    i += 2;
                } else if c == '/' && i + 1 < n && chars[i + 1] == '*' {
                    state = CommentState::Block;
                    out.push(' ');
                    i += 2;
                } else if c == '"' {
                    state = CommentState::Str;
                    out.push(c);
                    i += 1;
                } else if c == '\'' {
                    state = CommentState::Chr;
                    out.push(c);
                    i += 1;
                } else {
                    out.push(c);
                    i += 1;
                }
            }
            CommentState::Str | CommentState::Chr => {
                if c == '\n' {
                    state = CommentState::Normal;
                    out.push('\n');
                    i += 1;
                } else if c == '\\' && i + 1 < n {
                    out.push(c);
                    out.push(chars[i + 1]);
                    i += 2;
                } else {
                    let closing = matches!(
                        (state, c),
                        (CommentState::Str, '"') | (CommentState::Chr, '\'')
                    );
                    out.push(c);
                    if closing {
                        state = CommentState::Normal;
                    }
                    i += 1;
                }
            }
            CommentState::Line => {
                if c == '\n' {
                    state = CommentState::Normal;
                    out.push('\n');
                }
                i += 1;
            }
            CommentState::Block => {
                if c == '*' && i + 1 < n && chars[i + 1] == '/' {
                    state = CommentState::Normal;
                    out.push(' ');
                    i += 2;
                } else {
                    if c == '\n' {
                        out.push('\n');
                    }
                    i += 1;
                }
            }
        }
    }

    out
}

//
// `#if` constant-expression evaluation.
//
// Returns `Some(value)` when fully decided, `None` when "unknown" (which the
// caller treats as "traverse the branch"). Unknown propagates through operators
// except where `&&`/`||` can short-circuit.
//

#[derive(Clone, Debug)]
enum Tok {
    Num(i64),
    Ident(String),
    Op(&'static str),
    Unknown,
}

fn eval_expr(expr: &str, macros: &HashMap<String, String>) -> Option<i64> {
    let toks = resolve_tokens(&tokenize(expr), macros);
    let mut parser = ExprParser { toks: &toks, pos: 0 };
    parser.parse()
}

fn tokenize(expr: &str) -> Vec<Tok> {
    let chars: Vec<char> = expr.chars().collect();
    let n = chars.len();
    let mut toks = Vec::new();
    let mut i = 0;

    const OPS2: [&str; 9] = ["<<", ">>", "<=", ">=", "==", "!=", "&&", "||", "::"];
    const OPS1: [&str; 15] = [
        "!", "~", "*", "/", "%", "+", "-", "<", ">", "&", "^", "|", "?", ":", ",",
    ];

    while i < n {
        let c = chars[i];

        if c.is_whitespace() {
            i += 1;
            continue;
        }

        // Number literal.
        if c.is_ascii_digit() {
            let (value, next) = parse_number(&chars, i);
            toks.push(match value {
                Some(v) => Tok::Num(v),
                None => Tok::Unknown,
            });
            i = next;
            continue;
        }

        // Identifier (or keyword like `defined`).
        if c.is_ascii_alphabetic() || c == '_' {
            let start = i;
            while i < n && (chars[i].is_ascii_alphanumeric() || chars[i] == '_') {
                i += 1;
            }
            let ident: String = chars[start..i].iter().collect();
            toks.push(Tok::Ident(ident));
            continue;
        }

        // Char constant.
        if c == '\'' {
            let (value, next) = parse_char(&chars, i);
            toks.push(match value {
                Some(v) => Tok::Num(v),
                None => Tok::Unknown,
            });
            i = next;
            continue;
        }

        // String literal in a #if is unusual; treat as unknown and skip it.
        if c == '"' {
            i += 1;
            while i < n && chars[i] != '"' {
                if chars[i] == '\\' && i + 1 < n {
                    i += 2;
                } else {
                    i += 1;
                }
            }
            if i < n {
                i += 1;
            }
            toks.push(Tok::Unknown);
            continue;
        }

        if c == '(' {
            toks.push(Tok::Op("("));
            i += 1;
            continue;
        }
        if c == ')' {
            toks.push(Tok::Op(")"));
            i += 1;
            continue;
        }

        // Multi- then single-char operators.
        let two: String = chars[i..(i + 2).min(n)].iter().collect();
        if let Some(op) = OPS2.iter().find(|op| **op == two) {
            toks.push(Tok::Op(op));
            i += 2;
            continue;
        }
        let one = c.to_string();
        if let Some(op) = OPS1.iter().find(|op| **op == one) {
            toks.push(Tok::Op(op));
            i += 1;
            continue;
        }

        // Anything unrecognized: skip a char and mark unknown.
        toks.push(Tok::Unknown);
        i += 1;
    }

    toks
}

fn parse_number(chars: &[char], start: usize) -> (Option<i64>, usize) {
    let n = chars.len();
    let mut i = start;

    let (radix, digit_start) = if chars[i] == '0' && i + 1 < n && (chars[i + 1] == 'x' || chars[i + 1] == 'X') {
        (16, i + 2)
    } else if chars[i] == '0' {
        (8, i)
    } else {
        (10, i)
    };

    i = digit_start;
    let valid = |c: char, radix: u32| c.to_digit(radix).is_some();
    while i < n && valid(chars[i], radix) {
        i += 1;
    }
    let digits: String = chars[digit_start..i].iter().collect();

    // Skip integer suffixes (u/U/l/L).
    while i < n && matches!(chars[i], 'u' | 'U' | 'l' | 'L') {
        i += 1;
    }

    let value = if digits.is_empty() {
        // Lone `0` parsed as octal with empty body == 0.
        if radix == 8 {
            Some(0)
        } else {
            None
        }
    } else {
        i64::from_str_radix(&digits, radix).ok()
    };

    (value, i)
}

fn parse_char(chars: &[char], start: usize) -> (Option<i64>, usize) {
    let n = chars.len();
    let mut i = start + 1; // skip opening quote
    if i >= n {
        return (None, i);
    }

    let value = if chars[i] == '\\' && i + 1 < n {
        let escaped = chars[i + 1];
        i += 2;
        Some(match escaped {
            'n' => 10,
            't' => 9,
            'r' => 13,
            '0' => 0,
            other => other as i64,
        })
    } else {
        let v = chars[i] as i64;
        i += 1;
        Some(v)
    };

    // Skip to and past the closing quote.
    while i < n && chars[i] != '\'' {
        i += 1;
    }
    if i < n {
        i += 1;
    }
    (value, i)
}

/// Replace `defined X` / `defined(X)` with 1 (known) or Unknown, and substitute
/// known object-like macros' values (one level). Unknown identifiers become
/// `Tok::Unknown` rather than C's `0`, so they over-approximate to "traverse".
fn resolve_tokens(toks: &[Tok], macros: &HashMap<String, String>) -> Vec<Tok> {
    let mut out = Vec::new();
    let mut i = 0;

    while i < toks.len() {
        match &toks[i] {
            Tok::Ident(name) if name == "defined" => {
                let mut j = i + 1;
                let paren = matches!(toks.get(j), Some(Tok::Op("(")));
                if paren {
                    j += 1;
                }
                if let Some(Tok::Ident(id)) = toks.get(j) {
                    out.push(if macros.contains_key(id) {
                        Tok::Num(1)
                    } else {
                        Tok::Unknown
                    });
                    j += 1;
                    if paren && matches!(toks.get(j), Some(Tok::Op(")"))) {
                        j += 1;
                    }
                    i = j;
                } else {
                    out.push(Tok::Unknown);
                    i = j;
                }
            }
            Tok::Ident(name) => {
                match macros.get(name) {
                    Some(value) if !value.trim().is_empty() => {
                        // One-level substitution. Any identifiers inside the
                        // value become Unknown (we don't chase nested macros).
                        for t in tokenize(value) {
                            out.push(match t {
                                Tok::Ident(_) => Tok::Unknown,
                                other => other,
                            });
                        }
                    }
                    _ => out.push(Tok::Unknown),
                }
                i += 1;
            }
            other => {
                out.push(other.clone());
                i += 1;
            }
        }
    }

    out
}

struct ExprParser<'a> {
    toks: &'a [Tok],
    pos: usize,
}

impl ExprParser<'_> {
    fn peek_op(&self) -> Option<&'static str> {
        match self.toks.get(self.pos) {
            Some(Tok::Op(op)) => Some(op),
            _ => None,
        }
    }

    fn eat(&mut self, op: &str) -> bool {
        if self.peek_op() == Some(op) {
            self.pos += 1;
            true
        } else {
            false
        }
    }

    fn parse(&mut self) -> Option<i64> {
        self.ternary()
    }

    fn ternary(&mut self) -> Option<i64> {
        let cond = self.binary(0);
        if self.eat("?") {
            let then = self.ternary();
            self.eat(":");
            let otherwise = self.ternary();
            match cond {
                Some(0) => otherwise,
                Some(_) => then,
                None => {
                    if then == otherwise {
                        then
                    } else {
                        None
                    }
                }
            }
        } else {
            cond
        }
    }

    /// Precedence-climbing for binary operators. Higher `level` = tighter.
    fn binary(&mut self, level: usize) -> Option<i64> {
        const LEVELS: [&[&str]; 10] = [
            &["||"],
            &["&&"],
            &["|"],
            &["^"],
            &["&"],
            &["==", "!="],
            &["<", "<=", ">", ">="],
            &["<<", ">>"],
            &["+", "-"],
            &["*", "/", "%"],
        ];

        if level >= LEVELS.len() {
            return self.unary();
        }

        let mut left = self.binary(level + 1);
        while let Some(op) = self.peek_op() {
            if !LEVELS[level].contains(&op) {
                break;
            }
            self.pos += 1;
            let right = self.binary(level + 1);
            left = apply_binary(op, left, right);
        }
        left
    }

    fn unary(&mut self) -> Option<i64> {
        if self.eat("!") {
            return self.unary().map(|v| if v == 0 { 1 } else { 0 });
        }
        if self.eat("~") {
            return self.unary().map(|v| !v);
        }
        if self.eat("-") {
            return self.unary().map(|v| -v);
        }
        if self.eat("+") {
            return self.unary();
        }
        self.primary()
    }

    fn primary(&mut self) -> Option<i64> {
        match self.toks.get(self.pos) {
            Some(Tok::Num(v)) => {
                self.pos += 1;
                Some(*v)
            }
            Some(Tok::Unknown) => {
                self.pos += 1;
                None
            }
            Some(Tok::Op("(")) => {
                self.pos += 1;
                let inner = self.ternary();
                self.eat(")");
                inner
            }
            _ => {
                // Unexpected token; consume it to make progress.
                if self.pos < self.toks.len() {
                    self.pos += 1;
                }
                None
            }
        }
    }
}

fn apply_binary(op: &str, a: Option<i64>, b: Option<i64>) -> Option<i64> {
    // Short-circuiting logicals can decide a result with one known operand.
    match op {
        "&&" => {
            if a == Some(0) || b == Some(0) {
                return Some(0);
            }
            return match (a, b) {
                (Some(_), Some(_)) => Some(1),
                _ => None,
            };
        }
        "||" => {
            if matches!(a, Some(x) if x != 0) || matches!(b, Some(x) if x != 0) {
                return Some(1);
            }
            return match (a, b) {
                (Some(0), Some(0)) => Some(0),
                _ => None,
            };
        }
        _ => {}
    }

    let (a, b) = (a?, b?);
    let bool_to = |x: bool| if x { 1 } else { 0 };
    Some(match op {
        "|" => a | b,
        "^" => a ^ b,
        "&" => a & b,
        "==" => bool_to(a == b),
        "!=" => bool_to(a != b),
        "<" => bool_to(a < b),
        "<=" => bool_to(a <= b),
        ">" => bool_to(a > b),
        ">=" => bool_to(a >= b),
        "<<" => a.checked_shl(b as u32).unwrap_or(0),
        ">>" => a.checked_shr(b as u32).unwrap_or(0),
        "+" => a.wrapping_add(b),
        "-" => a.wrapping_sub(b),
        "*" => a.wrapping_mul(b),
        "/" => {
            if b == 0 {
                return None;
            }
            a.wrapping_div(b)
        }
        "%" => {
            if b == 0 {
                return None;
            }
            a.wrapping_rem(b)
        }
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn macros(pairs: &[(&str, &str)]) -> HashMap<String, String> {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    #[test]
    fn evaluates_constants() {
        let m = HashMap::new();
        assert_eq!(eval_expr("0", &m), Some(0));
        assert_eq!(eval_expr("1", &m), Some(1));
        assert_eq!(eval_expr("1 + 2 * 3", &m), Some(7));
        assert_eq!(eval_expr("(1 + 2) * 3", &m), Some(9));
        assert_eq!(eval_expr("1 << 4", &m), Some(16));
        assert_eq!(eval_expr("0x10 == 16", &m), Some(1));
        assert_eq!(eval_expr("5 > 3 && 2 < 4", &m), Some(1));
    }

    #[test]
    fn unknown_identifiers_are_unknown_not_zero() {
        let m = HashMap::new();
        // C would say 0; we say "unknown" so the branch is traversed.
        assert_eq!(eval_expr("FOO", &m), None);
        assert_eq!(eval_expr("FOO > 3", &m), None);
        // ...but short-circuit still decides what it can.
        assert_eq!(eval_expr("0 && FOO", &m), Some(0));
        assert_eq!(eval_expr("1 || FOO", &m), Some(1));
    }

    #[test]
    fn defined_operator() {
        let m = macros(&[("WIN32", "1")]);
        assert_eq!(eval_expr("defined(WIN32)", &m), Some(1));
        assert_eq!(eval_expr("defined WIN32", &m), Some(1));
        // Unknown macro: defined() is unknown (open-world), not 0.
        assert_eq!(eval_expr("defined(NOPE)", &m), None);
        assert_eq!(eval_expr("defined(WIN32) && 0", &m), Some(0));
    }

    #[test]
    fn macro_value_substitution() {
        let m = macros(&[("VER", "0x0600")]);
        assert_eq!(eval_expr("VER >= 0x0500", &m), Some(1));
        assert_eq!(eval_expr("VER == 1536", &m), Some(1));
    }

    #[test]
    fn strips_comments_and_splices() {
        let src = "a /* block\ncomment */ b // line\nc\\\nd";
        let lines = preprocess_text(src);
        assert_eq!(lines[0].trim(), "a");
        assert!(lines[1].trim().starts_with('b'));
        // Continuation joined "c" and "d" onto one logical line.
        assert!(lines.iter().any(|l| l.contains("cd")));
    }

    #[test]
    fn directive_parsing() {
        assert!(matches!(
            parse_directive("#include \"foo.h\""),
            Some(Directive::Include(_))
        ));
        assert!(matches!(
            parse_directive("  #  ifdef BAR"),
            Some(Directive::Ifdef(name)) if name == "BAR"
        ));
        // Non-directive code lines are not directives at all.
        assert!(parse_directive("int x = 1;").is_none());
        // #define/#undef are knowingly skipped, not "unknown".
        assert!(matches!(
            parse_directive("#define FOO 1"),
            Some(Directive::Ignored)
        ));
        assert!(matches!(
            parse_directive("#undef FOO"),
            Some(Directive::Ignored)
        ));
        // GNU line markers (`# 123 "file"`) are noise, not misses.
        assert!(matches!(
            parse_directive("# 1 \"foo.h\""),
            Some(Directive::Ignored)
        ));
        // Reviewed-as-irrelevant directives (diagnostics; include_next only ever
        // reaches system headers in this codebase) are knowingly ignored.
        for line in [
            "#error nope",
            "#warning hmm",
            "#line 42",
            "#include_next <foo.h>",
        ] {
            assert!(
                matches!(parse_directive(line), Some(Directive::Ignored)),
                "{line:?} should be Ignored"
            );
        }
        // A genuinely unrecognized directive is still surfaced as unknown.
        assert!(matches!(
            parse_directive("#import \"x.tlb\""),
            Some(Directive::Unknown { keyword }) if keyword == "import"
        ));
    }

    #[test]
    fn unknown_directives_are_reported() {
        let dir = std::env::temp_dir().join(format!("vc2ninja_pp_unk_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        std::fs::write(
            dir.join("root.cpp"),
            "#define X 1\n#import \"legacy.tlb\"\n#import \"other.tlb\"\n",
        )
        .unwrap();

        let mut cache = FileCache::default();
        let result =
            scan_translation_units(&[dir.join("root.cpp")], &[dir.clone()], &[], &mut cache);

        // #define is silent; #import is reported once (deduped by keyword).
        assert_eq!(result.unknown_directives.len(), 1);
        assert_eq!(result.unknown_directives[0].keyword, "import");
        assert!(result.unknown_directives[0].line.contains("legacy.tlb"));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn include_spec_parsing() {
        assert!(matches!(
            parse_include("\"a/b.h\""),
            Some((IncludeKind::Quote, n)) if n == "a/b.h"
        ));
        assert!(matches!(
            parse_include("<vector>"),
            Some((IncludeKind::Angle, n)) if n == "vector"
        ));
        // Computed include.
        assert!(parse_include("HEADER_MACRO").is_none());
    }

    #[test]
    fn pragma_comment_lib_parsing() {
        assert_eq!(
            parse_pragma_comment_lib("comment(lib, \"ws2_32.lib\")"),
            Some("ws2_32.lib".to_string())
        );
        assert_eq!(
            parse_pragma_comment_lib("comment ( lib , \"foo.lib\" )"),
            Some("foo.lib".to_string())
        );
        assert_eq!(parse_pragma_comment_lib("once"), None);
        assert_eq!(parse_pragma_comment_lib("comment(linker, \"/x\")"), None);
    }

    #[test]
    fn scans_transitive_headers_respecting_if_zero() {
        let dir = std::env::temp_dir().join(format!("vc2ninja_pp_test_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        std::fs::write(dir.join("a.h"), "#include \"b.h\"\n").unwrap();
        std::fs::write(dir.join("b.h"), "// leaf\n").unwrap();
        std::fs::write(dir.join("dead.h"), "// must not be picked up\n").unwrap();
        std::fs::write(
            dir.join("root.cpp"),
            "#include \"a.h\"\n#if 0\n#include \"dead.h\"\n#endif\n#pragma comment(lib, \"z.lib\")\n",
        )
        .unwrap();

        let mut cache = FileCache::default();
        let result = scan_translation_units(
            &[dir.join("root.cpp")],
            &[dir.clone()],
            &[],
            &mut cache,
        );

        let names: HashSet<String> = result
            .headers
            .iter()
            .map(|p| {
                p.file_name()
                    .unwrap()
                    .to_str()
                    .expect("file name is valid UTF-8")
                    .to_string()
            })
            .collect();

        assert!(names.contains("a.h"), "a.h should be a dependency");
        assert!(names.contains("b.h"), "b.h (transitive) should be a dependency");
        assert!(!names.contains("dead.h"), "#if 0 block must be pruned");
        assert_eq!(result.pragma_libs, vec!["z.lib".to_string()]);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn unknown_conditional_is_traversed() {
        let dir = std::env::temp_dir().join(format!("vc2ninja_pp_test2_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        std::fs::write(dir.join("maybe.h"), "// conditional leaf\n").unwrap();
        std::fs::write(
            dir.join("root.cpp"),
            "#ifdef SOME_UNKNOWN_MACRO\n#include \"maybe.h\"\n#endif\n",
        )
        .unwrap();

        let mut cache = FileCache::default();
        let result =
            scan_translation_units(&[dir.join("root.cpp")], &[dir.clone()], &[], &mut cache);

        let names: HashSet<String> = result
            .headers
            .iter()
            .map(|p| {
                p.file_name()
                    .unwrap()
                    .to_str()
                    .expect("file name is valid UTF-8")
                    .to_string()
            })
            .collect();
        assert!(
            names.contains("maybe.h"),
            "unknown #ifdef must be traversed (over-approximate)"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }
}
