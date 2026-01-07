//! Database-related performance rules.
//!
//! Detects N+1 query patterns and other database anti-patterns in Diesel, SQLx, and SeaORM.

use super::visitor::VisitorState;
use super::{Diagnostic, Rule, Severity};
use crate::engine::AnalysisContext;
use syn::punctuated::Punctuated;
use syn::visit::Visit;
use syn::{Expr, ExprAwait, ExprCall, ExprMethodCall, ExprPath, Token};

/// Detects N+1 query patterns - database queries executed inside loops.
///
/// This is a critical performance issue where instead of fetching related data
/// in a single batch query, code fetches one record at a time in a loop.
///
/// # Supported ORMs
/// - **SQLx**: `query`, `query_as`, `query_scalar` calls and `.fetch_*()` methods
/// - **Diesel**: `.load()`, `.first()`, `.get_result()`, `.execute()` methods
/// - **SeaORM**: `.find()`, `.one()`, `.all()`, Entity operations
///
/// # Example
/// ```rust,ignore
/// // Bad: N+1 query
/// for user_id in user_ids {
///     let posts = sqlx::query!("SELECT * FROM posts WHERE user_id = $1", user_id)
///         .fetch_all(&pool)
///         .await?;
/// }
///
/// // Good: Batch query
/// let posts = sqlx::query!("SELECT * FROM posts WHERE user_id = ANY($1)", &user_ids)
///     .fetch_all(&pool)
///     .await?;
/// ```
pub struct NPlusOneQueryRule;

impl Rule for NPlusOneQueryRule {
    fn id(&self) -> &'static str {
        "n-plus-one-query"
    }

    fn name(&self) -> &'static str {
        "N+1 Query Detection"
    }

    fn description(&self) -> &'static str {
        "Detects database queries inside loops that could be batched into a single query"
    }

    fn default_severity(&self) -> Severity {
        Severity::Error
    }

    fn check(&self, ctx: &AnalysisContext) -> Vec<Diagnostic> {
        let mut visitor = NPlusOneVisitor {
            ctx,
            diagnostics: Vec::new(),
            state: VisitorState::new(),
        };
        visitor.visit_file(ctx.ast);
        visitor.diagnostics
    }
}

struct NPlusOneVisitor<'a> {
    ctx: &'a AnalysisContext<'a>,
    diagnostics: Vec<Diagnostic>,
    state: VisitorState,
}

/// SQLx function calls that indicate a query
const SQLX_QUERY_FUNCTIONS: &[&str] = &[
    "query",
    "query_as",
    "query_scalar",
    "query_as_with",
    "query_scalar_with",
    "query_with",
];

/// SQLx method calls that execute a query - unambiguous, always flag
const SQLX_FETCH_METHODS: &[&str] = &[
    "fetch",
    "fetch_one",
    "fetch_all",
    "fetch_optional",
    "fetch_many",
];

/// Diesel method calls that are unambiguous
const DIESEL_UNAMBIGUOUS_METHODS: &[&str] = &["load", "get_result", "get_results"];

/// Diesel method calls that need receiver validation
const DIESEL_AMBIGUOUS_METHODS: &[&str] = &["first", "execute"];

/// SeaORM method calls that are unambiguous
const SEAORM_UNAMBIGUOUS_METHODS: &[&str] = &[
    "find_by_id",
    "find_related",
    "find_with_related",
    "find_also_related",
];

/// SeaORM method calls that need receiver validation
const SEAORM_AMBIGUOUS_METHODS: &[&str] = &["one", "all", "find"];

/// Entity operations that need receiver validation
const AMBIGUOUS_OPERATIONS: &[&str] = &["insert", "update", "delete", "save", "execute"];

/// Diesel table operations
const DIESEL_OPERATIONS: &[&str] = &["insert_into", "update", "delete"];

impl NPlusOneVisitor<'_> {
    fn report_diagnostic(&mut self, span: proc_macro2::Span, orm_hint: &str, pattern: &str) {
        let line = span.start().line;
        let column = span.start().column;

        self.diagnostics.push(Diagnostic {
            rule_id: "n-plus-one-query",
            severity: Severity::Error,
            message: format!(
                "Database query `{}` inside loop (N+1 query pattern). {}",
                pattern, orm_hint
            ),
            file_path: self.ctx.file_path.to_path_buf(),
            line,
            column,
            end_line: None,
            end_column: None,
            suggestion: Some(
                "Batch queries outside the loop using WHERE IN, ANY(), or join operations"
                    .to_string(),
            ),
            fix: None,
        });
    }

    fn check_sqlx_call(&mut self, path_str: &str, span: proc_macro2::Span) {
        // Check for sqlx::query* functions
        for &func in SQLX_QUERY_FUNCTIONS {
            if path_str.ends_with(func) {
                let prefix_len = path_str.len().saturating_sub(func.len());
                let is_boundary = prefix_len == 0 || path_str[..prefix_len].ends_with("::");
                if is_boundary {
                    self.report_diagnostic(
                        span,
                        "Consider using WHERE ... IN or ANY() for batch fetching.",
                        // cargo-perf-ignore: format-in-loop
                        &format!("sqlx::{}", func),
                    );
                    return;
                }
            }
        }
    }

    fn check_diesel_call(&mut self, path_str: &str, span: proc_macro2::Span) {
        // Check for diesel::insert_into, diesel::update, diesel::delete
        for &op in DIESEL_OPERATIONS {
            if path_str.ends_with(op) {
                let prefix_len = path_str.len().saturating_sub(op.len());
                let is_boundary = prefix_len == 0 || path_str[..prefix_len].ends_with("::");
                if is_boundary {
                    self.report_diagnostic(
                        span,
                        "Consider batch operations with insert_into().values(&vec_of_values).",
                        // cargo-perf-ignore: format-in-loop
                        &format!("diesel::{}", op),
                    );
                    return;
                }
            }
        }
    }

    fn check_method_call(
        &mut self,
        method_name: &str,
        span: proc_macro2::Span,
        receiver: &Expr,
        args: &Punctuated<Expr, Token![,]>,
    ) {
        // Check unambiguous methods first - always flag these (no false positives)
        if SQLX_FETCH_METHODS.contains(&method_name) {
            self.report_diagnostic(
                span,
                "Consider using WHERE ... IN or ANY() for batch fetching.",
                method_name,
            );
            return;
        }

        if DIESEL_UNAMBIGUOUS_METHODS.contains(&method_name) {
            self.report_diagnostic(
                span,
                "Consider using .filter(column.eq_any(&ids)) for batch operations.",
                method_name,
            );
            return;
        }

        if SEAORM_UNAMBIGUOUS_METHODS.contains(&method_name) {
            self.report_diagnostic(
                span,
                "Consider using Entity::find().filter(Column::Id.is_in(ids)) for batch fetching.",
                method_name,
            );
            return;
        }

        // For ambiguous methods, require validation to prevent false positives
        // (e.g., Vec::first(), HashMap::insert(), Iterator::find())
        // We check the receiver OR if args look like DB connection
        let receiver_looks_like_db = Self::looks_like_db_operation(receiver);
        let args_look_like_db = Self::has_db_connection_arg(args);

        if !receiver_looks_like_db && !args_look_like_db {
            return;
        }

        // Check ambiguous Diesel methods
        if DIESEL_AMBIGUOUS_METHODS.contains(&method_name) {
            self.report_diagnostic(
                span,
                "Consider using .filter(column.eq_any(&ids)) for batch operations.",
                method_name,
            );
            return;
        }

        // Check ambiguous SeaORM methods
        if SEAORM_AMBIGUOUS_METHODS.contains(&method_name) {
            self.report_diagnostic(
                span,
                "Consider using Entity::find().filter(Column::Id.is_in(ids)) for batch fetching.",
                method_name,
            );
            return;
        }

        // Check ambiguous operations (insert, update, delete, save)
        if AMBIGUOUS_OPERATIONS.contains(&method_name) {
            self.report_diagnostic(
                span,
                "Consider using Entity::insert_many() or batch operations.",
                method_name,
            );
        }
    }

    /// Check if any argument looks like a database connection.
    ///
    /// Database methods typically take a connection/pool as first argument.
    /// This helps identify patterns like `user.insert(db)` or `.one(&db)`.
    fn has_db_connection_arg(args: &Punctuated<Expr, Token![,]>) -> bool {
        const DB_CONNECTION_NAMES: &[&str] = &[
            "db",
            "conn",
            "connection",
            "pool",
            "database",
            "tx",
            "transaction",
        ];

        for arg in args.iter() {
            // Check path arguments: db, pool, conn, etc.
            if let Expr::Path(path) = arg {
                if let Some(ident) = path.path.get_ident() {
                    let name = ident.to_string().to_lowercase();
                    if DB_CONNECTION_NAMES
                        .iter()
                        .any(|&db_name| name.contains(db_name))
                    {
                        return true;
                    }
                }
            }
            // Check reference arguments: &db, &pool, &conn, etc.
            if let Expr::Reference(ref_expr) = arg {
                if let Expr::Path(path) = &*ref_expr.expr {
                    if let Some(ident) = path.path.get_ident() {
                        let name = ident.to_string().to_lowercase();
                        if DB_CONNECTION_NAMES
                            .iter()
                            .any(|&db_name| name.contains(db_name))
                        {
                            return true;
                        }
                    }
                }
            }
        }
        false
    }

    /// Check if the receiver expression looks like a database operation.
    ///
    /// Returns true if the expression appears to be:
    /// - A method chain containing database-related methods (query, find, filter, etc.)
    /// - A function call to a database function (sqlx::query, etc.)
    fn looks_like_db_operation(receiver: &Expr) -> bool {
        match receiver {
            // Check method chains: e.g., query(...).bind(...).fetch_one()
            Expr::MethodCall(method_call) => {
                let method_name = method_call.method.to_string();
                // Database query builder methods
                const DB_BUILDER_METHODS: &[&str] = &[
                    "query",
                    "query_as",
                    "query_scalar",
                    "find",
                    "find_by_id",
                    "filter",
                    "select",
                    "bind",
                    "execute",
                    "fetch",
                    "table",
                    "from",
                    "into",
                ];
                if DB_BUILDER_METHODS.contains(&method_name.as_str()) {
                    return true;
                }
                // Recursively check the receiver chain
                Self::looks_like_db_operation(&method_call.receiver)
            }
            // Check function calls: e.g., sqlx::query("..."), User::find_by_id(id)
            Expr::Call(call) => {
                if let Expr::Path(path) = &*call.func {
                    let path_str = path
                        .path
                        .segments
                        .iter()
                        .map(|s| s.ident.to_string())
                        .collect::<Vec<_>>()
                        .join("::");
                    // Check for known database function prefixes
                    if path_str.contains("sqlx")
                        || path_str.contains("query")
                        || path_str.contains("diesel")
                        || path_str.contains("Entity")
                    {
                        return true;
                    }
                    // Check for SeaORM-style <Entity>::find_by_id patterns
                    // The path often ends with a DB method name
                    let last_segment = path.path.segments.last().map(|s| s.ident.to_string());
                    if let Some(func_name) = last_segment {
                        const DB_FUNC_NAMES: &[&str] = &[
                            "find_by_id",
                            "find",
                            "insert",
                            "update",
                            "delete",
                            "insert_into",
                            "query",
                            "query_as",
                            "query_scalar",
                        ];
                        return DB_FUNC_NAMES.contains(&func_name.as_str());
                    }
                    false
                } else {
                    false
                }
            }
            // Check paths that look like database types: e.g., Entity::find_by_id
            Expr::Path(path) => {
                let path_str = path
                    .path
                    .segments
                    .iter()
                    .map(|s| s.ident.to_string())
                    .collect::<Vec<_>>()
                    .join("::");
                path_str.contains("Entity")
                    || path_str.contains("users")
                    || path_str.contains("table")
            }
            // Check await expressions (common in async db code)
            Expr::Await(await_expr) => Self::looks_like_db_operation(&await_expr.base),
            // Check try expressions (?)
            Expr::Try(try_expr) => Self::looks_like_db_operation(&try_expr.expr),
            // For other expressions, be conservative
            _ => false,
        }
    }
}

impl<'ast> Visit<'ast> for NPlusOneVisitor<'_> {
    fn visit_expr_for_loop(&mut self, node: &'ast syn::ExprForLoop) {
        if self.state.should_bail() {
            return;
        }
        self.state.enter_loop();
        syn::visit::visit_expr_for_loop(self, node);
        self.state.exit_loop();
    }

    fn visit_expr_while(&mut self, node: &'ast syn::ExprWhile) {
        if self.state.should_bail() {
            return;
        }
        self.state.enter_loop();
        syn::visit::visit_expr_while(self, node);
        self.state.exit_loop();
    }

    fn visit_expr_loop(&mut self, node: &'ast syn::ExprLoop) {
        if self.state.should_bail() {
            return;
        }
        self.state.enter_loop();
        syn::visit::visit_expr_loop(self, node);
        self.state.exit_loop();
    }

    fn visit_expr(&mut self, node: &'ast syn::Expr) {
        if self.state.should_bail() {
            return;
        }
        self.state.enter_expr();
        syn::visit::visit_expr(self, node);
        self.state.exit_expr();
    }

    fn visit_expr_call(&mut self, node: &'ast ExprCall) {
        if self.state.in_loop() {
            if let Expr::Path(ExprPath { path, .. }) = &*node.func {
                let path_str = path
                    .segments
                    .iter()
                    .map(|s| s.ident.to_string())
                    .collect::<Vec<_>>()
                    .join("::");

                // Check for SQLx calls
                self.check_sqlx_call(
                    &path_str,
                    path.segments
                        .last()
                        .map(|s| s.ident.span())
                        .unwrap_or_else(proc_macro2::Span::call_site),
                );

                // Check for Diesel calls
                self.check_diesel_call(
                    &path_str,
                    path.segments
                        .last()
                        .map(|s| s.ident.span())
                        .unwrap_or_else(proc_macro2::Span::call_site),
                );
            }
        }
        syn::visit::visit_expr_call(self, node);
    }

    fn visit_expr_method_call(&mut self, node: &'ast ExprMethodCall) {
        if self.state.in_loop() {
            let method_name = node.method.to_string();
            self.check_method_call(&method_name, node.method.span(), &node.receiver, &node.args);
        }
        syn::visit::visit_expr_method_call(self, node);
    }

    fn visit_expr_await(&mut self, node: &'ast ExprAwait) {
        // Continue visiting the base expression
        syn::visit::visit_expr_await(self, node);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::AnalysisContext;
    use crate::Config;
    use std::path::Path;

    fn check_code(source: &str) -> Vec<Diagnostic> {
        let ast = syn::parse_file(source).expect("Failed to parse test code");
        let config = Config::default();
        let ctx = AnalysisContext::new(Path::new("test.rs"), source, &ast, &config);
        NPlusOneQueryRule.check(&ctx)
    }

    // SQLx tests
    #[test]
    fn test_detects_sqlx_query_in_loop() {
        let source = r#"
            async fn bad(pool: &PgPool, ids: Vec<i32>) {
                for id in ids {
                    let _ = sqlx::query("SELECT * FROM users WHERE id = $1")
                        .bind(id)
                        .fetch_one(pool)
                        .await;
                }
            }
        "#;
        let diagnostics = check_code(source);
        assert!(!diagnostics.is_empty());
        assert!(diagnostics
            .iter()
            .any(|d| d.message.contains("sqlx::query")));
    }

    #[test]
    fn test_detects_sqlx_query_as_in_loop() {
        let source = r#"
            async fn bad(pool: &PgPool, ids: Vec<i32>) {
                for id in ids {
                    let _ = sqlx::query_as::<_, User>("SELECT * FROM users WHERE id = $1")
                        .bind(id)
                        .fetch_one(pool)
                        .await;
                }
            }
        "#;
        let diagnostics = check_code(source);
        assert!(!diagnostics.is_empty());
        assert!(diagnostics.iter().any(|d| d.message.contains("query_as")));
    }

    #[test]
    fn test_detects_fetch_one_in_loop() {
        let source = r#"
            async fn bad(pool: &PgPool, ids: Vec<i32>) {
                for id in ids {
                    let query = build_query(id);
                    let _ = query.fetch_one(pool).await;
                }
            }
        "#;
        let diagnostics = check_code(source);
        assert!(!diagnostics.is_empty());
        assert!(diagnostics.iter().any(|d| d.message.contains("fetch_one")));
    }

    #[test]
    fn test_detects_fetch_all_in_loop() {
        let source = r#"
            async fn bad(pool: &PgPool, user_ids: Vec<i32>) {
                for user_id in user_ids {
                    let posts = query.fetch_all(pool).await;
                }
            }
        "#;
        let diagnostics = check_code(source);
        assert!(!diagnostics.is_empty());
        assert!(diagnostics.iter().any(|d| d.message.contains("fetch_all")));
    }

    // Diesel tests
    #[test]
    fn test_detects_diesel_load_in_loop() {
        let source = r#"
            fn bad(conn: &mut PgConnection, ids: Vec<i32>) {
                for id in ids {
                    let _ = users::table
                        .filter(users::id.eq(id))
                        .load::<User>(conn);
                }
            }
        "#;
        let diagnostics = check_code(source);
        assert!(!diagnostics.is_empty());
        assert!(diagnostics.iter().any(|d| d.message.contains("load")));
    }

    #[test]
    fn test_detects_diesel_first_in_loop() {
        let source = r#"
            fn bad(conn: &mut PgConnection, ids: Vec<i32>) {
                for id in ids {
                    let _ = users::table.first::<User>(conn);
                }
            }
        "#;
        let diagnostics = check_code(source);
        assert!(!diagnostics.is_empty());
        assert!(diagnostics.iter().any(|d| d.message.contains("first")));
    }

    #[test]
    fn test_detects_diesel_get_result_in_loop() {
        let source = r#"
            fn bad(conn: &mut PgConnection, users: Vec<NewUser>) {
                for user in users {
                    diesel::insert_into(users::table)
                        .values(&user)
                        .get_result::<User>(conn);
                }
            }
        "#;
        let diagnostics = check_code(source);
        assert!(!diagnostics.is_empty());
    }

    #[test]
    fn test_detects_diesel_insert_into_in_loop() {
        let source = r#"
            fn bad(conn: &mut PgConnection, new_users: Vec<NewUser>) {
                for user in new_users {
                    diesel::insert_into(users::table)
                        .values(&user)
                        .execute(conn);
                }
            }
        "#;
        let diagnostics = check_code(source);
        assert!(!diagnostics.is_empty());
        assert!(diagnostics
            .iter()
            .any(|d| d.message.contains("insert_into") || d.message.contains("execute")));
    }

    // SeaORM tests
    #[test]
    fn test_detects_seaorm_find_by_id_in_loop() {
        let source = r#"
            async fn bad(db: &DatabaseConnection, ids: Vec<i32>) {
                for id in ids {
                    let _ = User::find_by_id(id).one(db).await;
                }
            }
        "#;
        let diagnostics = check_code(source);
        assert!(!diagnostics.is_empty());
        // Should detect both find_by_id and one
        assert!(diagnostics
            .iter()
            .any(|d| d.message.contains("find_by_id") || d.message.contains("one")));
    }

    #[test]
    fn test_detects_seaorm_find_in_loop() {
        let source = r#"
            async fn bad(db: &DatabaseConnection, ids: Vec<i32>) {
                for id in ids {
                    let _ = User::find()
                        .filter(user::Column::Id.eq(id))
                        .all(db)
                        .await;
                }
            }
        "#;
        let diagnostics = check_code(source);
        assert!(!diagnostics.is_empty());
    }

    #[test]
    fn test_detects_seaorm_insert_in_loop() {
        let source = r#"
            async fn bad(db: &DatabaseConnection, users: Vec<user::ActiveModel>) {
                for user in users {
                    user.insert(db).await;
                }
            }
        "#;
        let diagnostics = check_code(source);
        assert!(!diagnostics.is_empty());
        assert!(diagnostics.iter().any(|d| d.message.contains("insert")));
    }

    // Negative tests
    #[test]
    fn test_no_detection_query_outside_loop() {
        let source = r#"
            async fn good(pool: &PgPool, ids: Vec<i32>) {
                let users = sqlx::query("SELECT * FROM users WHERE id = ANY($1)")
                    .bind(&ids)
                    .fetch_all(pool)
                    .await;

                for user in users {
                    println!("{:?}", user);
                }
            }
        "#;
        let diagnostics = check_code(source);
        assert!(diagnostics.is_empty());
    }

    #[test]
    fn test_no_detection_batch_diesel_outside_loop() {
        let source = r#"
            fn good(conn: &mut PgConnection, ids: Vec<i32>) {
                let users = users::table
                    .filter(users::id.eq_any(&ids))
                    .load::<User>(conn);

                for user in users {
                    println!("{:?}", user);
                }
            }
        "#;
        let diagnostics = check_code(source);
        assert!(diagnostics.is_empty());
    }

    #[test]
    fn test_no_detection_batch_insert_diesel() {
        let source = r#"
            fn good(conn: &mut PgConnection, new_users: Vec<NewUser>) {
                diesel::insert_into(users::table)
                    .values(&new_users)
                    .execute(conn);
            }
        "#;
        let diagnostics = check_code(source);
        assert!(diagnostics.is_empty());
    }

    #[test]
    fn test_detects_query_in_while_loop() {
        let source = r#"
            async fn bad(pool: &PgPool) {
                let mut cursor = get_cursor();
                while let Some(id) = cursor.next() {
                    let _ = sqlx::query("SELECT * FROM users WHERE id = $1")
                        .fetch_one(pool)
                        .await;
                }
            }
        "#;
        let diagnostics = check_code(source);
        assert!(!diagnostics.is_empty());
    }

    #[test]
    fn test_detects_query_in_loop_loop() {
        let source = r#"
            async fn bad(pool: &PgPool, ids: &mut impl Iterator<Item = i32>) {
                loop {
                    let Some(id) = ids.next() else { break };
                    let _ = sqlx::query("SELECT * FROM users WHERE id = $1")
                        .fetch_one(pool)
                        .await;
                }
            }
        "#;
        let diagnostics = check_code(source);
        assert!(!diagnostics.is_empty());
    }

    #[test]
    fn test_detects_query_in_nested_loop() {
        let source = r#"
            async fn bad(pool: &PgPool, groups: Vec<Vec<i32>>) {
                for group in groups {
                    for id in group {
                        let _ = sqlx::query("SELECT * FROM users WHERE id = $1")
                            .fetch_one(pool)
                            .await;
                    }
                }
            }
        "#;
        let diagnostics = check_code(source);
        assert!(!diagnostics.is_empty());
    }

    #[test]
    fn test_severity_is_error() {
        let rule = NPlusOneQueryRule;
        assert_eq!(rule.default_severity(), Severity::Error);
    }

    #[test]
    fn test_rule_metadata() {
        let rule = NPlusOneQueryRule;
        assert_eq!(rule.id(), "n-plus-one-query");
        assert_eq!(rule.name(), "N+1 Query Detection");
        assert!(!rule.description().is_empty());
    }
}
