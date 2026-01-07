import * as vscode from 'vscode';
import * as path from 'path';
import {
    LanguageClient,
    LanguageClientOptions,
    ServerOptions,
    TransportKind
} from 'vscode-languageclient/node';

let client: LanguageClient | undefined;

export function activate(context: vscode.ExtensionContext) {
    const config = vscode.workspace.getConfiguration('cargo-perf');

    if (!config.get<boolean>('enable', true)) {
        return;
    }

    // Start the LSP client
    startClient(context);

    // Register commands
    context.subscriptions.push(
        vscode.commands.registerCommand('cargo-perf.analyze', runAnalysis),
        vscode.commands.registerCommand('cargo-perf.fix', runFix)
    );
}

function startClient(context: vscode.ExtensionContext) {
    const config = vscode.workspace.getConfiguration('cargo-perf');
    const command = config.get<string>('path', 'cargo-perf');

    const serverOptions: ServerOptions = {
        command,
        args: ['lsp'],
        transport: TransportKind.stdio
    };

    const clientOptions: LanguageClientOptions = {
        documentSelector: [{ scheme: 'file', language: 'rust' }],
        synchronize: {
            fileEvents: vscode.workspace.createFileSystemWatcher('**/*.rs')
        }
    };

    client = new LanguageClient(
        'cargo-perf',
        'cargo-perf Language Server',
        serverOptions,
        clientOptions
    );

    client.start();
}

async function runAnalysis() {
    const config = vscode.workspace.getConfiguration('cargo-perf');
    const command = config.get<string>('path', 'cargo-perf');
    const strict = config.get<boolean>('strict', false);

    const workspaceFolders = vscode.workspace.workspaceFolders;
    if (!workspaceFolders) {
        vscode.window.showErrorMessage('No workspace folder open');
        return;
    }

    const terminal = vscode.window.createTerminal('cargo-perf');
    terminal.show();

    const args = strict ? ['check', '--strict'] : ['check'];
    terminal.sendText(`${command} ${args.join(' ')}`);
}

async function runFix() {
    const config = vscode.workspace.getConfiguration('cargo-perf');
    const command = config.get<string>('path', 'cargo-perf');

    const result = await vscode.window.showWarningMessage(
        'This will modify files. Continue?',
        'Yes',
        'Dry Run',
        'Cancel'
    );

    if (result === 'Cancel' || !result) {
        return;
    }

    const terminal = vscode.window.createTerminal('cargo-perf');
    terminal.show();

    const args = result === 'Dry Run' ? ['fix', '--dry-run'] : ['fix'];
    terminal.sendText(`${command} ${args.join(' ')}`);
}

export function deactivate(): Thenable<void> | undefined {
    if (!client) {
        return undefined;
    }
    return client.stop();
}
