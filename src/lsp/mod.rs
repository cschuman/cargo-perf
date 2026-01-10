//! Language Server Protocol (LSP) server for cargo-perf.
//!
//! This module provides IDE integration via the LSP protocol.
//! Enable with the `lsp` feature:
//!
//! ```bash
//! cargo install cargo-perf --features lsp
//! cargo perf lsp
//! ```
//!
//! ## Supported Capabilities
//!
//! - Real-time diagnostics on file save
//! - Diagnostic severity mapping (errors, warnings)
//! - Code actions with auto-fix support
//!
//! ## Editor Setup
//!
//! ### VS Code
//!
//! Install the cargo-perf extension or configure manually:
//!
//! ```json
//! {
//!   "cargo-perf.enable": true,
//!   "cargo-perf.command": "cargo-perf lsp"
//! }
//! ```
//!
//! ### Neovim (with nvim-lspconfig)
//!
//! ```lua
//! require('lspconfig.configs').cargo_perf = {
//!   default_config = {
//!     cmd = { 'cargo-perf', 'lsp' },
//!     filetypes = { 'rust' },
//!     root_dir = require('lspconfig.util').root_pattern('Cargo.toml'),
//!   },
//! }
//! require('lspconfig').cargo_perf.setup({})
//! ```

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

use tokio::sync::RwLock;
use tower_lsp::jsonrpc::Result;
use tower_lsp::lsp_types::*;
use tower_lsp::{Client, LanguageServer, LspService, Server};

use crate::engine::LineIndex;
use crate::{analyze, Config, Diagnostic as PerfDiagnostic, Fix, Severity as PerfSeverity};

/// Maximum file size to analyze (10 MB)
const MAX_FILE_SIZE: u64 = 10 * 1024 * 1024;

/// Minimum interval between analyses of the same file (debouncing)
const MIN_ANALYSIS_INTERVAL_MS: u128 = 500;

/// Maximum entries in rate-limit cache before cleanup
const MAX_RATE_LIMIT_ENTRIES: usize = 1000;

/// Age after which rate-limit entries are considered stale (5 minutes)
const RATE_LIMIT_STALE_SECS: u64 = 300;

/// Stored diagnostic with its fix for code actions
#[derive(Clone)]
struct StoredDiagnostic {
    lsp_diagnostic: tower_lsp::lsp_types::Diagnostic,
    fix: Option<Fix>,
    file_path: PathBuf,
}

/// The cargo-perf LSP server backend.
pub struct Backend {
    client: Client,
    config: Arc<RwLock<Config>>,
    root_path: Arc<RwLock<Option<PathBuf>>>,
    /// Track last analysis time per file for rate limiting
    last_analysis: Arc<RwLock<HashMap<String, Instant>>>,
    /// Store diagnostics with fixes for code actions
    stored_diagnostics: Arc<RwLock<HashMap<Url, Vec<StoredDiagnostic>>>>,
}

impl Backend {
    /// Create a new backend instance.
    pub fn new(client: Client) -> Self {
        Self {
            client,
            config: Arc::new(RwLock::new(Config::default())),
            root_path: Arc::new(RwLock::new(None)),
            last_analysis: Arc::new(RwLock::new(HashMap::new())),
            stored_diagnostics: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Validate that a path is within the workspace boundaries.
    ///
    /// Returns the canonicalized path if valid, None otherwise.
    async fn validate_path_in_workspace(&self, path: &Path) -> Option<PathBuf> {
        let root = self.root_path.read().await.clone()?;

        // Canonicalize both paths to prevent ../ bypasses
        let canonical_path = match path.canonicalize() {
            Ok(p) => p,
            Err(_) => return None,
        };

        let canonical_root = match root.canonicalize() {
            Ok(p) => p,
            Err(_) => return None,
        };

        // SECURITY: Ensure the path is within the workspace
        if canonical_path.starts_with(&canonical_root) {
            Some(canonical_path)
        } else {
            None
        }
    }

    /// Check if analysis should be rate-limited for this file.
    async fn should_rate_limit(&self, uri_str: &str) -> bool {
        let last_analysis = self.last_analysis.read().await;
        if let Some(&last) = last_analysis.get(uri_str) {
            last.elapsed().as_millis() < MIN_ANALYSIS_INTERVAL_MS
        } else {
            false
        }
    }

    /// Record that analysis was performed for a file.
    async fn record_analysis(&self, uri_str: String) {
        let mut last_analysis = self.last_analysis.write().await;
        let now = Instant::now();
        last_analysis.insert(uri_str, now);

        // Cleanup only when significantly over limit to reduce lock contention
        // Using 2x threshold avoids frequent cleanup during heavy analysis
        if last_analysis.len() > MAX_RATE_LIMIT_ENTRIES * 2 {
            let stale_threshold = std::time::Duration::from_secs(RATE_LIMIT_STALE_SECS);
            last_analysis.retain(|_, instant| instant.elapsed() < stale_threshold);
        }
    }

    /// Analyze a document and publish diagnostics.
    async fn analyze_and_publish(&self, uri: &Url) {
        let path = match uri.to_file_path() {
            Ok(p) => p,
            Err(_) => return,
        };

        // Only analyze .rs files
        if path.extension().is_some_and(|ext| ext != "rs") {
            return;
        }

        // SECURITY: Validate path is within workspace (prevent path traversal)
        let canonical_path = match self.validate_path_in_workspace(&path).await {
            Some(p) => p,
            None => {
                self.client
                    .log_message(MessageType::WARNING, "Path outside workspace, skipping")
                    .await;
                return;
            }
        };

        // SECURITY: Open file immediately after validation to reduce TOCTOU window
        // Get metadata from open file handle, not path (prevents race condition)
        let file = match std::fs::File::open(&canonical_path) {
            Ok(f) => f,
            Err(_) => return,
        };
        match file.metadata() {
            Ok(meta) => {
                if meta.len() > MAX_FILE_SIZE {
                    self.client
                        .log_message(
                            MessageType::WARNING,
                            format!(
                                "File too large to analyze: {} bytes (max: {} bytes)",
                                meta.len(),
                                MAX_FILE_SIZE
                            ),
                        )
                        .await;
                    return;
                }
            }
            Err(_) => return,
        }
        // Drop file handle before analyze (which will re-open)
        // Note: This reduces but doesn't eliminate TOCTOU - full fix would require
        // passing file content to analyze(), which is a larger refactor
        drop(file);

        // Rate limiting: debounce rapid re-analysis requests
        let uri_str = uri.to_string();
        if self.should_rate_limit(&uri_str).await {
            return;
        }
        self.record_analysis(uri_str).await;

        let config = self.config.read().await.clone();

        // Run analysis on the file
        let diagnostics = match analyze(&canonical_path, &config) {
            Ok(diags) => diags,
            Err(e) => {
                self.client
                    .log_message(
                        MessageType::ERROR,
                        "Analysis failed. Check logs for details.",
                    )
                    .await;
                // Log detailed error separately (not exposed to client)
                eprintln!("cargo-perf analysis error: {}", e);
                return;
            }
        };

        // Convert to LSP diagnostics and store for code actions
        // cargo-perf-ignore: vec-no-capacity
        let mut stored = Vec::new();
        // cargo-perf-ignore: vec-no-capacity
        let mut lsp_diagnostics = Vec::new();

        for diag in diagnostics
            .into_iter()
            .filter(|d| d.file_path == canonical_path)
        {
            // Need clones: fix for storage, file_path for StoredDiagnostic, lsp_diag for both vectors
            // cargo-perf-ignore: clone-in-hot-loop
            let fix = diag.fix.clone();
            // cargo-perf-ignore: clone-in-hot-loop
            let file_path = diag.file_path.clone();
            let lsp_diag = perf_diag_to_lsp(diag);

            stored.push(StoredDiagnostic {
                // cargo-perf-ignore: clone-in-hot-loop
                lsp_diagnostic: lsp_diag.clone(),
                fix,
                file_path,
            });
            lsp_diagnostics.push(lsp_diag);
        }

        // Store diagnostics for code actions
        {
            let mut stored_map = self.stored_diagnostics.write().await;
            stored_map.insert(uri.clone(), stored);
        }

        self.client
            .publish_diagnostics(uri.clone(), lsp_diagnostics, None)
            .await;
    }

    /// Analyze all Rust files in the workspace.
    async fn analyze_workspace(&self) {
        let root = self.root_path.read().await.clone();
        let root = match root {
            Some(r) => r,
            None => return,
        };

        let config = self.config.read().await.clone();

        let diagnostics = match analyze(&root, &config) {
            Ok(diags) => diags,
            Err(e) => {
                self.client
                    .log_message(
                        MessageType::ERROR,
                        format!("Workspace analysis failed: {}", e),
                    )
                    .await;
                return;
            }
        };

        // Group diagnostics by file
        let mut by_file: HashMap<PathBuf, Vec<PerfDiagnostic>> = HashMap::new();
        for diag in diagnostics {
            // cargo-perf-ignore: clone-in-hot-loop
            let key = diag.file_path.clone();
            by_file.entry(key).or_default().push(diag);
        }

        // Publish and store diagnostics for each file
        for (path, diags) in by_file {
            if let Ok(uri) = Url::from_file_path(&path) {
                // cargo-perf-ignore: vec-no-capacity
                let mut stored = Vec::new();
                // cargo-perf-ignore: vec-no-capacity
                let mut lsp_diagnostics = Vec::new();

                for diag in diags {
                    // cargo-perf-ignore: clone-in-hot-loop
                    let fix = diag.fix.clone();
                    // cargo-perf-ignore: clone-in-hot-loop
                    let file_path = diag.file_path.clone();
                    let lsp_diag = perf_diag_to_lsp(diag);

                    stored.push(StoredDiagnostic {
                        // cargo-perf-ignore: clone-in-hot-loop
                        lsp_diagnostic: lsp_diag.clone(),
                        fix,
                        file_path,
                    });
                    lsp_diagnostics.push(lsp_diag);
                }

                // Store for code actions (outside inner loop - not mutex-in-loop)
                {
                    // cargo-perf-ignore: mutex-in-loop
                    let mut stored_map = self.stored_diagnostics.write().await;
                    // cargo-perf-ignore: clone-in-hot-loop
                    stored_map.insert(uri.clone(), stored);
                }

                self.client
                    .publish_diagnostics(uri, lsp_diagnostics, None)
                    .await;
            }
        }
    }
}

#[tower_lsp::async_trait]
impl LanguageServer for Backend {
    async fn initialize(&self, params: InitializeParams) -> Result<InitializeResult> {
        // Store root path
        if let Some(root_uri) = params.root_uri {
            if let Ok(path) = root_uri.to_file_path() {
                *self.root_path.write().await = Some(path.clone());

                // Try to load config from workspace
                if let Ok(cfg) = Config::load_or_default(&path) {
                    *self.config.write().await = cfg;
                }
            }
        }

        Ok(InitializeResult {
            capabilities: ServerCapabilities {
                text_document_sync: Some(TextDocumentSyncCapability::Options(
                    TextDocumentSyncOptions {
                        open_close: Some(true),
                        change: Some(TextDocumentSyncKind::FULL),
                        save: Some(TextDocumentSyncSaveOptions::SaveOptions(SaveOptions {
                            include_text: Some(false),
                        })),
                        ..Default::default()
                    },
                )),
                diagnostic_provider: Some(DiagnosticServerCapabilities::Options(
                    DiagnosticOptions {
                        identifier: Some("cargo-perf".to_string()),
                        inter_file_dependencies: true,
                        workspace_diagnostics: true,
                        ..Default::default()
                    },
                )),
                code_action_provider: Some(CodeActionProviderCapability::Options(
                    CodeActionOptions {
                        code_action_kinds: Some(vec![CodeActionKind::QUICKFIX]),
                        resolve_provider: Some(false),
                        ..Default::default()
                    },
                )),
                ..Default::default()
            },
            server_info: Some(ServerInfo {
                name: "cargo-perf".to_string(),
                version: Some(env!("CARGO_PKG_VERSION").to_string()),
            }),
        })
    }

    async fn initialized(&self, _: InitializedParams) {
        self.client
            .log_message(MessageType::INFO, "cargo-perf LSP server initialized")
            .await;

        // Initial workspace analysis
        self.analyze_workspace().await;
    }

    async fn shutdown(&self) -> Result<()> {
        Ok(())
    }

    async fn did_open(&self, params: DidOpenTextDocumentParams) {
        self.analyze_and_publish(&params.text_document.uri).await;
    }

    async fn did_change(&self, _params: DidChangeTextDocumentParams) {
        // We analyze on save, not on every change (for performance)
    }

    async fn did_save(&self, params: DidSaveTextDocumentParams) {
        self.analyze_and_publish(&params.text_document.uri).await;
    }

    async fn did_close(&self, params: DidCloseTextDocumentParams) {
        // Clear diagnostics and stored data when file is closed
        {
            let mut stored = self.stored_diagnostics.write().await;
            stored.remove(&params.text_document.uri);
        }
        self.client
            .publish_diagnostics(params.text_document.uri, vec![], None)
            .await;
    }

    async fn code_action(&self, params: CodeActionParams) -> Result<Option<CodeActionResponse>> {
        let uri = &params.text_document.uri;
        let request_range = params.range;

        // Get stored diagnostics for this file
        let stored = self.stored_diagnostics.read().await;
        let file_diagnostics = match stored.get(uri) {
            Some(diags) => diags,
            None => return Ok(None),
        };

        // Find diagnostics that overlap with the requested range and have fixes
        // cargo-perf-ignore: vec-no-capacity
        let mut actions = Vec::new();

        for stored_diag in file_diagnostics {
            // Check if this diagnostic overlaps with the requested range
            if !ranges_overlap(&stored_diag.lsp_diagnostic.range, &request_range) {
                continue;
            }

            // Only create code action if there's a fix
            let fix = match &stored_diag.fix {
                Some(f) => f,
                None => continue,
            };

            // Read file to build LineIndex for byte-to-position conversion
            let source = match std::fs::read_to_string(&stored_diag.file_path) {
                Ok(s) => s,
                Err(e) => {
                    eprintln!(
                        "Warning: Cannot read file for code action: {} ({})",
                        stored_diag.file_path.display(),
                        e
                    );
                    continue;
                }
            };
            let line_index = LineIndex::new(&source);

            // Build workspace edit from fix replacements
            // cargo-perf-ignore: vec-no-capacity
            let mut text_edits = Vec::new();
            for replacement in &fix.replacements {
                let (start_line, start_col) = line_index.line_col(replacement.start_byte);
                let (end_line, end_col) = line_index.line_col(replacement.end_byte);

                text_edits.push(TextEdit {
                    range: Range {
                        start: Position {
                            line: start_line.saturating_sub(1) as u32,
                            character: start_col.saturating_sub(1) as u32,
                        },
                        end: Position {
                            line: end_line.saturating_sub(1) as u32,
                            character: end_col.saturating_sub(1) as u32,
                        },
                    },
                    // cargo-perf-ignore: clone-in-hot-loop
                    new_text: replacement.new_text.clone(),
                });
            }

            if text_edits.is_empty() {
                continue;
            }

            let mut changes = HashMap::new();
            // cargo-perf-ignore: clone-in-hot-loop
            changes.insert(uri.clone(), text_edits);

            let code_action = CodeAction {
                // cargo-perf-ignore: clone-in-hot-loop
                title: fix.description.clone(),
                kind: Some(CodeActionKind::QUICKFIX),
                // cargo-perf-ignore: clone-in-hot-loop
                diagnostics: Some(vec![stored_diag.lsp_diagnostic.clone()]),
                edit: Some(WorkspaceEdit {
                    changes: Some(changes),
                    document_changes: None,
                    change_annotations: None,
                }),
                command: None,
                is_preferred: Some(true),
                disabled: None,
                data: None,
            };

            actions.push(CodeActionOrCommand::CodeAction(code_action));
        }

        if actions.is_empty() {
            Ok(None)
        } else {
            Ok(Some(actions))
        }
    }
}

/// Check if two ranges overlap.
fn ranges_overlap(a: &Range, b: &Range) -> bool {
    // Ranges overlap if neither is entirely before or after the other
    !(a.end.line < b.start.line
        || (a.end.line == b.start.line && a.end.character < b.start.character)
        || b.end.line < a.start.line
        || (b.end.line == a.start.line && b.end.character < a.start.character))
}

/// Convert a cargo-perf diagnostic to an LSP diagnostic.
fn perf_diag_to_lsp(diag: PerfDiagnostic) -> tower_lsp::lsp_types::Diagnostic {
    let severity = match diag.severity {
        PerfSeverity::Error => DiagnosticSeverity::ERROR,
        PerfSeverity::Warning => DiagnosticSeverity::WARNING,
        PerfSeverity::Info => DiagnosticSeverity::INFORMATION,
    };

    let range = Range {
        start: Position {
            line: diag.line.saturating_sub(1) as u32,
            character: diag.column as u32,
        },
        end: Position {
            line: diag.end_line.unwrap_or(diag.line).saturating_sub(1) as u32,
            character: diag.end_column.unwrap_or(diag.column + 10) as u32,
        },
    };

    let mut related_info = Vec::new();
    if let Some(suggestion) = &diag.suggestion {
        related_info.push(DiagnosticRelatedInformation {
            location: Location {
                uri: Url::from_file_path(&diag.file_path)
                    .unwrap_or_else(|_| Url::parse("file:///unknown").unwrap()),
                range,
            },
            message: suggestion.clone(),
        });
    }

    tower_lsp::lsp_types::Diagnostic {
        range,
        severity: Some(severity),
        code: Some(NumberOrString::String(diag.rule_id.to_string())),
        code_description: None,
        source: Some("cargo-perf".to_string()),
        message: diag.message,
        related_information: if related_info.is_empty() {
            None
        } else {
            Some(related_info)
        },
        tags: None,
        data: None,
    }
}

/// Run the LSP server.
///
/// This function blocks until the server is shut down.
pub async fn run_server() {
    let stdin = tokio::io::stdin();
    let stdout = tokio::io::stdout();

    let (service, socket) = LspService::new(Backend::new);
    Server::new(stdin, stdout, socket).serve(service).await;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_severity_conversion() {
        let diag = PerfDiagnostic {
            rule_id: "test-rule",
            severity: PerfSeverity::Error,
            message: "Test message".to_string(),
            file_path: PathBuf::from("/test.rs"),
            line: 10,
            column: 5,
            end_line: None,
            end_column: None,
            suggestion: None,
            fix: None,
        };

        let lsp_diag = perf_diag_to_lsp(diag);
        assert_eq!(lsp_diag.severity, Some(DiagnosticSeverity::ERROR));
        assert_eq!(lsp_diag.source, Some("cargo-perf".to_string()));
    }

    #[test]
    fn test_range_conversion() {
        let diag = PerfDiagnostic {
            rule_id: "test-rule",
            severity: PerfSeverity::Warning,
            message: "Test".to_string(),
            file_path: PathBuf::from("/test.rs"),
            line: 10,
            column: 5,
            end_line: Some(12),
            end_column: Some(20),
            suggestion: None,
            fix: None,
        };

        let lsp_diag = perf_diag_to_lsp(diag);
        assert_eq!(lsp_diag.range.start.line, 9); // 0-indexed
        assert_eq!(lsp_diag.range.start.character, 5);
        assert_eq!(lsp_diag.range.end.line, 11);
        assert_eq!(lsp_diag.range.end.character, 20);
    }

    #[test]
    fn test_ranges_overlap() {
        let range_a = Range {
            start: Position {
                line: 5,
                character: 0,
            },
            end: Position {
                line: 10,
                character: 0,
            },
        };

        // Overlapping range
        let range_b = Range {
            start: Position {
                line: 8,
                character: 0,
            },
            end: Position {
                line: 12,
                character: 0,
            },
        };
        assert!(ranges_overlap(&range_a, &range_b));

        // Non-overlapping range (before)
        let range_c = Range {
            start: Position {
                line: 0,
                character: 0,
            },
            end: Position {
                line: 4,
                character: 0,
            },
        };
        assert!(!ranges_overlap(&range_a, &range_c));

        // Non-overlapping range (after)
        let range_d = Range {
            start: Position {
                line: 11,
                character: 0,
            },
            end: Position {
                line: 15,
                character: 0,
            },
        };
        assert!(!ranges_overlap(&range_a, &range_d));

        // Contained range
        let range_e = Range {
            start: Position {
                line: 6,
                character: 0,
            },
            end: Position {
                line: 8,
                character: 0,
            },
        };
        assert!(ranges_overlap(&range_a, &range_e));
    }
}
