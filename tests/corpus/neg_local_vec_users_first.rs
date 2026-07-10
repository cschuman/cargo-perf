// perf-guard: n-plus-one-query
// Negative (D30): a local `Vec` named `users`; `users.first()` is the standard
// slice method. A path receiver's NAME is no longer treated as database lineage, so
// a variable called `users` no longer corroborates a Diesel N+1.
struct User {
    name: String,
}

fn greet(groups: &[Vec<User>]) {
    for group in groups {
        let users = group;
        if let Some(u) = users.first() {
            println!("{}", u.name);
        }
    }
}
