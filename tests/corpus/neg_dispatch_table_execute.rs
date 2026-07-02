// perf-guard: n-plus-one-query
// Negative (D31): a routing/dispatch struct in a local named `table`;
// `table.execute(cmd)` is a plain in-memory call. A path receiver named `table` is
// no longer treated as database lineage, so this dispatch loop is not an N+1.
struct Command;
struct DispatchTable;
impl DispatchTable {
    fn execute(&self, _cmd: &Command) {}
}

fn run(cmds: &[Command]) {
    let table = DispatchTable;
    for cmd in cmds {
        table.execute(cmd);
    }
}
