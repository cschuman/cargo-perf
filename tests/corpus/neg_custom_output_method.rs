// perf-guard: async-block-in-async
// Negative: `.output()` on a custom builder is not `std::process::Command`.
// Matching blocking calls by bare method name is a false positive.
struct ReportBuilder;

impl ReportBuilder {
    fn output(&self) -> String {
        String::new()
    }
}

async fn make() -> String {
    ReportBuilder.output()
}
