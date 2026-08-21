use crate::parse::CliCommand;

pub const ROOT_HELP: &str = "\
incodex — Incognito toggle for Codex desktop

Usage:
  incodex                     Interactive menu (terminal only)
  incodex <command> [flags]

Commands:
  install      Patch the Codex app you are using
  uninstall    Restore the official Codex app
  status       Show whether Incodex is installed
  doctor       Diagnose the install
  runtime      Update Incodex without re-signing Codex
  recover      Roll back a failed install
  open         Open an incognito window without patching Codex
  update       Update this CLI
  self-uninstall  Remove this CLI (not Codex, unless --restore-app)

Run incodex <command> --help for details.
inc is the same program as incodex.
";

pub fn command_help(command: CliCommand) -> &'static str {
    match command {
        CliCommand::Install => {
            "\
Usage:
  incodex install [flags]

Patch Codex. With no flags this is the app at /Applications/ChatGPT.app.

Flags:
  --yes            Skip the confirmation prompt (required when stdin is not a terminal)
  --dry-run, -n    Print the plan and exit
  --clone          Patch a copy at ~/.incodex/scratch/ChatGPT.app
  --app <path>     Patch a specific .app

Examples:
  incodex install
  incodex install --yes
  incodex install --dry-run
  incodex install --clone
"
        }
        CliCommand::Uninstall => {
            "\
Usage:
  incodex uninstall [flags]

Restore Codex to the snapshot taken at install. With no flags this is
/Applications/ChatGPT.app.

Flags:
  --yes            Skip the confirmation prompt (required when stdin is not a terminal)
  --dry-run, -n    Print the plan and exit
  --clone          Restore ~/.incodex/scratch/ChatGPT.app
  --app <path>     Restore a specific .app

Examples:
  incodex uninstall
  incodex uninstall --yes
  incodex uninstall --dry-run
"
        }
        CliCommand::Status => {
            "\
Usage:
  incodex status [--json] [--app <path>]

Show whether Incodex is installed in Codex.

Examples:
  incodex status
  incodex status --json
"
        }
        CliCommand::Doctor => {
            "\
Usage:
  incodex doctor [--json] [--deep] [--app <path>]

Diagnose the install, Runtime files, backup, journals, and leftover sessions.
The default checks Incodex-owned state and minimal app identity evidence.
Use --deep to inspect nested signing, entitlements, and Gatekeeper.

Flags:
  --deep            Inspect nested signing, entitlements, and Gatekeeper

Examples:
  incodex doctor
  incodex doctor --json
  incodex doctor --deep
"
        }
        CliCommand::Runtime => {
            "\
Usage:
  incodex runtime

Write Incodex's own code to ~/.incodex/runtime/. Does not modify Codex.
Reopen Codex to load it.

Examples:
  incodex runtime
"
        }
        CliCommand::Recover => {
            "\
Usage:
  incodex recover --transaction <id>

Roll back an install that stopped halfway. Uncommitted work is never continued.

Examples:
  incodex recover --transaction <id>
"
        }
        CliCommand::Open => {
            "\
Usage:
  incodex open [--dry-run] [--app <path>]

Open an incognito window without patching Codex. Uses an isolated CODEX_HOME
and Chromium user-data-dir. The hat-glasses control and banner still appear
in that window. Closing the window burns that session.

Examples:
  incodex open
  incodex open --dry-run
"
        }
        CliCommand::Update => {
            "\
Usage:
  incodex update [--dry-run]

Update the CLI. Script installs re-run install.sh. Homebrew installs should
use brew upgrade incodex. Source checkouts should git pull.

Examples:
  incodex update
  incodex update --dry-run
"
        }
        CliCommand::SelfUninstall => {
            "\
Usage:
  incodex self-uninstall [--restore-app] [--yes] [--dry-run]

Remove the CLI from PATH. Does not restore Codex unless --restore-app.

Examples:
  incodex self-uninstall
  incodex self-uninstall --restore-app --yes
  incodex self-uninstall --dry-run
"
        }
        CliCommand::Menu | CliCommand::Help | CliCommand::Version => ROOT_HELP,
    }
}
