-- cargo-perf LSP configuration for Neovim
--
-- Installation:
-- 1. Install cargo-perf with LSP feature:
--    cargo install cargo-perf --features lsp
--
-- 2. Add this file to your Neovim config or copy the contents to init.lua
--
-- Usage with nvim-lspconfig:
--
-- require('lspconfig.configs').cargo_perf = {
--   default_config = {
--     cmd = { 'cargo-perf', 'lsp' },
--     filetypes = { 'rust' },
--     root_dir = require('lspconfig.util').root_pattern('Cargo.toml'),
--   },
-- }
-- require('lspconfig').cargo_perf.setup({})

local M = {}

-- Setup function for manual configuration
function M.setup(opts)
    opts = opts or {}

    -- Validate cargo-perf is available
    local handle = io.popen("cargo-perf --version 2>/dev/null")
    if handle then
        local result = handle:read("*a")
        handle:close()
        if result == "" then
            vim.notify("cargo-perf not found. Install with: cargo install cargo-perf --features lsp", vim.log.levels.WARN)
            return
        end
    end

    -- Configure LSP client
    local client_id = vim.lsp.start({
        name = 'cargo-perf',
        cmd = opts.cmd or { 'cargo-perf', 'lsp' },
        root_dir = opts.root_dir or vim.fs.dirname(vim.fs.find({ 'Cargo.toml' }, { upward = true })[1]),
        capabilities = opts.capabilities or vim.lsp.protocol.make_client_capabilities(),
    })

    if client_id then
        vim.notify("cargo-perf LSP started", vim.log.levels.INFO)
    end
end

-- Command to run cargo-perf check
function M.check(args)
    args = args or {}
    local cmd = "cargo perf check"
    if args.strict then
        cmd = cmd .. " --strict"
    end

    vim.fn.jobstart(cmd, {
        on_stdout = function(_, data)
            for _, line in ipairs(data) do
                if line ~= "" then
                    print(line)
                end
            end
        end,
        on_stderr = function(_, data)
            for _, line in ipairs(data) do
                if line ~= "" then
                    vim.notify(line, vim.log.levels.ERROR)
                end
            end
        end,
    })
end

-- Command to run cargo-perf fix
function M.fix(args)
    args = args or {}
    local cmd = "cargo perf fix"
    if args.dry_run then
        cmd = cmd .. " --dry-run"
    end

    vim.fn.jobstart(cmd, {
        on_stdout = function(_, data)
            for _, line in ipairs(data) do
                if line ~= "" then
                    print(line)
                end
            end
        end,
        on_exit = function(_, code)
            if code == 0 and not args.dry_run then
                vim.cmd("checktime")  -- Reload buffers
                vim.notify("Fixes applied. Buffers reloaded.", vim.log.levels.INFO)
            end
        end,
    })
end

-- Register user commands
vim.api.nvim_create_user_command('CargoPerfCheck', function(opts)
    M.check({ strict = opts.bang })
end, { bang = true, desc = 'Run cargo-perf check (! for strict mode)' })

vim.api.nvim_create_user_command('CargoPerfFix', function(opts)
    M.fix({ dry_run = opts.bang })
end, { bang = true, desc = 'Run cargo-perf fix (! for dry run)' })

return M

-- Example setup in init.lua:
--
-- -- Option 1: Using nvim-lspconfig (recommended)
-- require('lspconfig.configs').cargo_perf = {
--   default_config = {
--     cmd = { 'cargo-perf', 'lsp' },
--     filetypes = { 'rust' },
--     root_dir = require('lspconfig.util').root_pattern('Cargo.toml'),
--     settings = {},
--   },
-- }
-- require('lspconfig').cargo_perf.setup({})
--
-- -- Option 2: Manual setup
-- require('cargo-perf').setup()
--
-- -- Keybindings
-- vim.keymap.set('n', '<leader>pc', ':CargoPerfCheck<CR>', { desc = 'cargo-perf check' })
-- vim.keymap.set('n', '<leader>pf', ':CargoPerfFix<CR>', { desc = 'cargo-perf fix' })
