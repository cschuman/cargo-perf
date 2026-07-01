// Negative: a single clone in straight-line code is not a hot-loop clone.
fn once(s: &String) -> String {
    let copy = s.clone();
    copy
}
