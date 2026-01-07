;;; cargo-perf.el --- Integration with cargo-perf for Rust performance linting -*- lexical-binding: t -*-

;; Copyright (C) 2024 cargo-perf authors

;; Author: cargo-perf
;; Version: 0.4.0
;; Package-Requires: ((emacs "27.1") (lsp-mode "8.0"))
;; Keywords: languages, rust, performance, linting
;; URL: https://github.com/cschuman/cargo-perf

;;; Commentary:

;; This package provides integration between cargo-perf and Emacs.
;; It supports both LSP mode and Flycheck.

;; Installation:
;; 1. Install cargo-perf with LSP feature:
;;    cargo install cargo-perf --features lsp
;;
;; 2. Add this file to your load-path
;; 3. Add to your config:
;;    (require 'cargo-perf)
;;    (cargo-perf-setup)

;;; Code:

(require 'lsp-mode nil t)

(defgroup cargo-perf nil
  "cargo-perf integration for Emacs."
  :prefix "cargo-perf-"
  :group 'tools)

(defcustom cargo-perf-command "cargo-perf"
  "Path to the cargo-perf executable."
  :type 'string
  :group 'cargo-perf)

(defcustom cargo-perf-strict nil
  "When non-nil, run cargo-perf in strict mode."
  :type 'boolean
  :group 'cargo-perf)

;; LSP client registration
(when (featurep 'lsp-mode)
  (lsp-register-client
   (make-lsp-client
    :new-connection (lsp-stdio-connection
                     (lambda ()
                       (list cargo-perf-command "lsp")))
    :major-modes '(rust-mode rust-ts-mode)
    :priority -1  ; Lower priority than rust-analyzer
    :server-id 'cargo-perf
    :add-on? t)))  ; Run alongside rust-analyzer

;;;###autoload
(defun cargo-perf-setup ()
  "Set up cargo-perf LSP client."
  (interactive)
  (when (featurep 'lsp-mode)
    (add-to-list 'lsp-language-id-configuration '(rust-mode . "rust"))
    (add-to-list 'lsp-language-id-configuration '(rust-ts-mode . "rust"))
    (message "cargo-perf LSP client registered")))

;;;###autoload
(defun cargo-perf-check ()
  "Run cargo-perf check on the current project."
  (interactive)
  (let ((default-directory (or (locate-dominating-file default-directory "Cargo.toml")
                                default-directory)))
    (compile (concat cargo-perf-command " check"
                     (when cargo-perf-strict " --strict")))))

;;;###autoload
(defun cargo-perf-fix (&optional dry-run)
  "Run cargo-perf fix on the current project.
With prefix argument DRY-RUN, only show what would be fixed."
  (interactive "P")
  (let ((default-directory (or (locate-dominating-file default-directory "Cargo.toml")
                                default-directory)))
    (compile (concat cargo-perf-command " fix"
                     (when dry-run " --dry-run")))))

;;;###autoload
(defun cargo-perf-fix-dry-run ()
  "Run cargo-perf fix in dry-run mode."
  (interactive)
  (cargo-perf-fix t))

;; Flycheck integration (optional)
(when (featurep 'flycheck)
  (require 'flycheck)

  (flycheck-define-checker cargo-perf
    "A Rust performance linter using cargo-perf."
    :command ("cargo-perf" "check" "--format" "json")
    :error-parser flycheck-parse-json
    :error-patterns
    ((warning line-start
              (file-name) ":" line ":" column ": "
              (message) line-end))
    :modes (rust-mode rust-ts-mode)
    :predicate (lambda ()
                 (locate-dominating-file default-directory "Cargo.toml")))

  (add-to-list 'flycheck-checkers 'cargo-perf))

(provide 'cargo-perf)

;;; cargo-perf.el ends here

;; Example configuration in init.el:
;;
;; ;; Using use-package
;; (use-package cargo-perf
;;   :after (lsp-mode rust-mode)
;;   :config
;;   (cargo-perf-setup)
;;   :bind (:map rust-mode-map
;;          ("C-c p c" . cargo-perf-check)
;;          ("C-c p f" . cargo-perf-fix)))
;;
;; ;; Or manual setup
;; (require 'cargo-perf)
;; (cargo-perf-setup)
;; (define-key rust-mode-map (kbd "C-c p c") #'cargo-perf-check)
;; (define-key rust-mode-map (kbd "C-c p f") #'cargo-perf-fix)
