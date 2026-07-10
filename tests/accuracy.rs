//! Accuracy scorecard: precision / recall over a labeled corpus.
//!
//! This is cargo-perf's "prove it" test. Every fixture under `tests/corpus/` is
//! analyzed by *every* registered rule, and the results are scored against
//! hand-labeled ground truth:
//!
//! * A source line carrying a `// perf-expect: <rule-id>[, <rule-id>...]` marker
//!   asserts the tool must report those rules on that line (true positives).
//! * Any reported diagnostic with no matching marker is a **false positive**.
//! * Any marker with no matching diagnostic is a **false negative**.
//! * A fixture with no markers is a negative case whose entire job is to stay
//!   silent — it directly guards the false-positive fixes (Arc/Rc clones,
//!   `io::Read`/`io::Write` in loops, async guards dropped before `.await`, ...).
//!
//! Markers are inline (co-located with the code) rather than in sidecar files so
//! that inserting a line in a fixture can never desynchronize the expectation
//! from the code it describes.
//!
//! The aggregate precision and recall are printed as a scorecard and enforced
//! against committed floors. Cases the tool does not yet handle correctly belong
//! under `tests/corpus/known_gaps/` (tracked, not scored) so the floor stays
//! honest.

use cargo_perf::engine::parser::parse_file;
use cargo_perf::engine::AnalysisContext;
use cargo_perf::rules::registry;
use cargo_perf::Config;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

/// Committed accuracy floors — a one-way ratchet, now at the ceiling.
///
/// After the adversarial FP/FN hunt and its 10 remediation batches, this curated
/// corpus scores a clean 1.00 / 1.00, so the floor is set to match: on this
/// corpus the tool must be *perfect*, and any regression — a new false positive
/// or a lost true positive — fails CI rather than silently eroding the score.
///
/// Two deliberate design choices keep a 1.00 floor honest rather than brittle:
///   * Cases the tool does not yet get right are NOT counted against the floor;
///     they live under `tests/corpus/known_gaps/` (tracked, unscored) with an
///     explicit promotion path. That directory — not a fractional floor — is the
///     honest valve for "we know, and here's the shape of it."
///   * The floor is enforced PER RULE as well as in aggregate, and every rule
///     currently rests on a thin corpus (often a single TP), so recall is
///     effectively quantized to {0.00, 1.00}. A fractional floor would buy no
///     real headroom and would only mask a genuine miss; the fix for thin-rule
///     brittleness is to GROW the corpus, never to lower the bar.
///
/// Raising the corpus's coverage is welcome; lowering these numbers is a
/// regression that must be justified in review.
const MIN_PRECISION: f64 = 1.00;
const MIN_RECALL: f64 = 1.00;

const EXPECT_MARKER: &str = "// perf-expect:";
const GUARD_MARKER: &str = "// perf-guard:";

type Finding = (String, usize);

/// Parse `// perf-expect: <rule-id>[, ...]` markers into `(rule_id, line)` pairs.
fn expected_findings(source: &str) -> BTreeSet<Finding> {
    let mut set = BTreeSet::new();
    for (idx, line) in source.lines().enumerate() {
        if let Some(pos) = line.find(EXPECT_MARKER) {
            let rest = &line[pos + EXPECT_MARKER.len()..];
            for rule_id in rest.split(',') {
                let rule_id = rule_id.trim();
                if !rule_id.is_empty() {
                    set.insert((rule_id.to_string(), idx + 1));
                }
            }
        }
    }
    set
}

/// Run every registered rule over `source`, returning the `(rule_id, line)` of
/// each diagnostic. Panics if a fixture fails to parse — fixtures must be valid
/// Rust so that any finding is attributable to a rule, not to a parse gap.
fn actual_findings(path: &Path, source: &str) -> BTreeSet<Finding> {
    let ast = parse_file(source)
        .unwrap_or_else(|e| panic!("fixture {} must be valid Rust: {e}", path.display()));
    let config = Config::default();
    let ctx = AnalysisContext::new(path, source, &ast, &config);
    let mut set = BTreeSet::new();
    for rule in registry::all_rules() {
        for diag in rule.check(&ctx) {
            set.insert((diag.rule_id.to_string(), diag.line));
        }
    }
    set
}

/// Parse `// perf-guard: <rule-id>[, ...]` markers: the rule ids this fixture is
/// a declared negative (false-positive) guard for. The fixture must stay silent
/// for those rules (enforced by the scorecard's no-marker == false-positive rule).
fn guard_markers(source: &str) -> BTreeSet<String> {
    let mut set = BTreeSet::new();
    for line in source.lines() {
        if let Some(pos) = line.find(GUARD_MARKER) {
            for rule_id in line[pos + GUARD_MARKER.len()..].split(',') {
                let rule_id = rule_id.trim();
                if !rule_id.is_empty() {
                    set.insert(rule_id.to_string());
                }
            }
        }
    }
    set
}

/// Collect corpus fixtures, skipping the `known_gaps/` tree.
fn corpus_fixtures() -> Vec<PathBuf> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/corpus");
    let mut files = Vec::new();
    for entry in walkdir::WalkDir::new(&root)
        .into_iter()
        .filter_map(Result::ok)
    {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("rs") {
            continue;
        }
        if path.components().any(|c| c.as_os_str() == "known_gaps") {
            continue;
        }
        files.push(path.to_path_buf());
    }
    files.sort();
    files
}

#[derive(Default, Clone, Copy)]
struct Counts {
    tp: usize,
    fp: usize,
    fn_: usize,
}

fn precision(c: Counts) -> f64 {
    let denom = c.tp + c.fp;
    if denom == 0 {
        1.0
    } else {
        c.tp as f64 / denom as f64
    }
}

fn recall(c: Counts) -> f64 {
    let denom = c.tp + c.fn_;
    if denom == 0 {
        1.0
    } else {
        c.tp as f64 / denom as f64
    }
}

#[test]
fn accuracy_scorecard_meets_floor() {
    let fixtures = corpus_fixtures();
    assert!(
        !fixtures.is_empty(),
        "no corpus fixtures found under tests/corpus/"
    );

    let mut per_rule: BTreeMap<String, Counts> = BTreeMap::new();
    let mut overall = Counts::default();
    let mut failures: Vec<String> = Vec::new();

    // Optional authoring aid: `PERF_CORPUS_DUMP=1 cargo test --test accuracy -- --nocapture`
    let dump = std::env::var_os("PERF_CORPUS_DUMP").is_some();

    for path in &fixtures {
        let source = std::fs::read_to_string(path).expect("read fixture");
        let expected = expected_findings(&source);
        let actual = actual_findings(path, &source);

        if dump {
            eprintln!("--- {}", path.display());
            for (rule, line) in &actual {
                eprintln!("  actual: {rule} @ {line}");
            }
        }

        let name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("<fixture>");

        for f in expected.intersection(&actual) {
            per_rule.entry(f.0.clone()).or_default().tp += 1;
            overall.tp += 1;
        }
        for f in actual.difference(&expected) {
            per_rule.entry(f.0.clone()).or_default().fp += 1;
            overall.fp += 1;
            failures.push(format!(
                "FALSE POSITIVE: {} reported `{}` at line {} (no marker)",
                name, f.0, f.1
            ));
        }
        for f in expected.difference(&actual) {
            per_rule.entry(f.0.clone()).or_default().fn_ += 1;
            overall.fn_ += 1;
            failures.push(format!(
                "FALSE NEGATIVE: {} expected `{}` at line {} (not reported)",
                name, f.0, f.1
            ));
        }
    }

    // --- Scorecard -------------------------------------------------------
    let mut report = String::new();
    report.push_str("\ncargo-perf accuracy scorecard\n");
    report.push_str(&format!(
        "  fixtures: {}   |   floors: precision >= {:.2}, recall >= {:.2}\n",
        fixtures.len(),
        MIN_PRECISION,
        MIN_RECALL
    ));
    report.push_str("  ------------------------------------------------------------\n");
    report.push_str("  rule                        TP  FP  FN   prec   recall\n");
    for (rule, c) in &per_rule {
        report.push_str(&format!(
            "  {:<26} {:>3} {:>3} {:>3}  {:>5.2}  {:>5.2}\n",
            rule,
            c.tp,
            c.fp,
            c.fn_,
            precision(*c),
            recall(*c)
        ));
    }
    report.push_str("  ------------------------------------------------------------\n");
    report.push_str(&format!(
        "  OVERALL                    {:>3} {:>3} {:>3}  {:>5.2}  {:>5.2}\n",
        overall.tp,
        overall.fp,
        overall.fn_,
        precision(overall),
        recall(overall)
    ));
    println!("{report}");
    eprintln!("{report}");

    // Per-rule floor: a single rule's false positives must not be able to hide
    // behind the aggregate. Every rule that fired at all must clear the floor.
    for (rule, c) in &per_rule {
        let pr = precision(*c);
        let rc = recall(*c);
        if pr < MIN_PRECISION || rc < MIN_RECALL {
            failures.push(format!(
                "RULE BELOW FLOOR: {rule} precision {pr:.2} (>= {MIN_PRECISION:.2}?) recall {rc:.2} (>= {MIN_RECALL:.2}?)"
            ));
        }
    }

    let p = precision(overall);
    let r = recall(overall);

    assert!(
        failures.is_empty() && p >= MIN_PRECISION && r >= MIN_RECALL,
        "accuracy floor not met (precision {:.3} >= {:.2}? recall {:.3} >= {:.2}?):\n{}{}",
        p,
        MIN_PRECISION,
        r,
        MIN_RECALL,
        report,
        if failures.is_empty() {
            String::new()
        } else {
            format!("\n{}\n", failures.join("\n"))
        }
    );
}

/// Coverage gate: every registered rule must be exercised by BOTH a positive
/// fixture (a `// perf-expect:` that proves it fires when it should) AND a
/// negative fixture (a `// perf-guard:` that proves it stays silent when it
/// shouldn't). This is what keeps the scorecard from being vacuous: a rule with
/// no positive fixture could regress to never-firing undetected, and a rule with
/// no negative fixture has no false-positive floor at all.
#[test]
fn every_rule_has_positive_and_negative_fixtures() {
    let fixtures = corpus_fixtures();
    let mut positives: BTreeSet<String> = BTreeSet::new();
    let mut guards: BTreeSet<String> = BTreeSet::new();
    for path in &fixtures {
        let source = std::fs::read_to_string(path).expect("read fixture");
        for (rule, _) in expected_findings(&source) {
            positives.insert(rule);
        }
        for rule in guard_markers(&source) {
            guards.insert(rule);
        }
    }

    let mut missing = Vec::new();
    for rule in registry::all_rules() {
        let id = rule.id().to_string();
        if !positives.contains(&id) {
            missing.push(format!("  {id}: no positive fixture (// perf-expect: {id})"));
        }
        if !guards.contains(&id) {
            missing.push(format!("  {id}: no negative fixture (// perf-guard: {id})"));
        }
    }

    assert!(
        missing.is_empty(),
        "every registered rule needs a positive AND a negative fixture; missing:\n{}",
        missing.join("\n")
    );
}
