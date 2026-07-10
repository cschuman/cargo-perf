// Positive: with `use std::process::Command;` a bare `Command::new(..).output()`
// is a real blocking call reached via the use-map. The import oracle resolves
// the unqualified `Command` back to std::process::Command, so it must still fire
// (the mirror of the neg_user_struct_command guard, which shadows the name).
use std::process::Command;

async fn run() {
    let _ = Command::new("ls").output(); // perf-expect: async-block-in-async
}
