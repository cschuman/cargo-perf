// perf-guard: async-block-in-async
// Negative (D8): `tokio::process::Command::new(..).output().await` IS the
// recommended async form — the exact fix this rule suggests. Flagging it would
// mean the linter fires on its own advice, so it must stay silent.
async fn run() -> std::process::Output {
    tokio::process::Command::new("ls")
        .output()
        .await
        .unwrap()
}
