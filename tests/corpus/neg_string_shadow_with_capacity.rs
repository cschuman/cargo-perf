// perf-guard: string-no-capacity
// Negative (D34): `line` = String::new() is shadowed by
// `line` = String::with_capacity(1024), which is the binding actually grown in the
// loop. The stale `String::new()` classification must clear on the shadowing rebind.
fn build_csv(rows: &[Vec<String>]) -> String {
    let line = String::new();
    let _ = &line;
    let mut line = String::with_capacity(1024);
    for cell in rows.iter().flatten() {
        line.push_str(cell);
        line.push(',');
    }
    line
}
