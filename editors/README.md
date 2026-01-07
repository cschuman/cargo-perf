# Editor Integration

cargo-perf provides IDE integration via the Language Server Protocol (LSP).

## Installation

First, install cargo-perf with LSP support:

```bash
cargo install cargo-perf --features lsp
```

Verify it works:

```bash
cargo-perf lsp --help
```

## VS Code

### Using the Extension

1. Install the extension from `editors/vscode/`:
   ```bash
   cd editors/vscode
   npm install
   npm run compile
   code --install-extension .
   ```

2. The extension will automatically start the LSP server for Rust files.

### Manual Configuration

If you prefer manual setup, add to `.vscode/settings.json`:

```json
{
  "cargo-perf.enable": true,
  "cargo-perf.strict": false
}
```

## Neovim

### Using nvim-lspconfig (Recommended)

Add to your Neovim config:

```lua
-- Register cargo-perf as an LSP server
require('lspconfig.configs').cargo_perf = {
  default_config = {
    cmd = { 'cargo-perf', 'lsp' },
    filetypes = { 'rust' },
    root_dir = require('lspconfig.util').root_pattern('Cargo.toml'),
  },
}

-- Enable it
require('lspconfig').cargo_perf.setup({})
```

### Using the Plugin File

1. Copy `editors/neovim/cargo-perf.lua` to your config directory
2. Require it in your init.lua:

```lua
require('cargo-perf').setup()

-- Optional keybindings
vim.keymap.set('n', '<leader>pc', ':CargoPerfCheck<CR>', { desc = 'cargo-perf check' })
vim.keymap.set('n', '<leader>pf', ':CargoPerfFix<CR>', { desc = 'cargo-perf fix' })
```

## Emacs

### Using lsp-mode

1. Copy `editors/emacs/cargo-perf.el` to your load path
2. Add to your config:

```elisp
(require 'cargo-perf)
(cargo-perf-setup)

;; Optional keybindings
(define-key rust-mode-map (kbd "C-c p c") #'cargo-perf-check)
(define-key rust-mode-map (kbd "C-c p f") #'cargo-perf-fix)
```

### Using use-package

```elisp
(use-package cargo-perf
  :after (lsp-mode rust-mode)
  :config
  (cargo-perf-setup)
  :bind (:map rust-mode-map
         ("C-c p c" . cargo-perf-check)
         ("C-c p f" . cargo-perf-fix)))
```

## Helix

Add to `~/.config/helix/languages.toml`:

```toml
[[language]]
name = "rust"
language-servers = ["rust-analyzer", "cargo-perf"]

[language-server.cargo-perf]
command = "cargo-perf"
args = ["lsp"]
```

## Zed

Add to your Zed settings:

```json
{
  "lsp": {
    "cargo-perf": {
      "binary": {
        "path": "cargo-perf",
        "arguments": ["lsp"]
      }
    }
  },
  "languages": {
    "Rust": {
      "language_servers": ["rust-analyzer", "cargo-perf"]
    }
  }
}
```

## Generic LSP Client

For any editor with LSP support, configure:

- **Command**: `cargo-perf lsp`
- **File types**: `rust`
- **Root pattern**: `Cargo.toml`

## Troubleshooting

### LSP Server Not Starting

1. Verify cargo-perf is in your PATH:
   ```bash
   which cargo-perf
   cargo-perf --version
   ```

2. Check if LSP feature is enabled:
   ```bash
   cargo-perf lsp --help
   ```
   If this fails, reinstall with `--features lsp`.

3. Check LSP logs in your editor for error messages.

### No Diagnostics Appearing

1. Ensure the file is saved (diagnostics update on save)
2. Check if the file is in a valid Cargo project
3. Verify `cargo-perf.toml` isn't disabling rules

### Diagnostics Not Updating

cargo-perf analyzes on save, not on every keystroke. Save the file to trigger analysis.

## Running Alongside rust-analyzer

cargo-perf is designed to complement rust-analyzer, not replace it. Most editors support running multiple LSP servers for the same language:

- **VS Code**: Both run automatically
- **Neovim**: Configure both in lspconfig
- **Emacs**: Set cargo-perf as an add-on LSP (`add-on? t`)
- **Helix/Zed**: List both in language server configuration
