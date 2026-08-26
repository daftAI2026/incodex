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

#[cfg(target_os = "windows")]
pub const WINDOWS_ROOT_HELP: &str = "\
incodex — Incognito toggle for Codex desktop

Windows support uses the current user's official Microsoft Store Codex package.
The installed package is never patched or copied.

Usage:
  incodex                     Interactive menu (terminal only)
  incodex <command> [flags]

Commands:
  open         Open an isolated incognito Codex window
  status       Show official Store package availability
  doctor       Diagnose package health and isolated sessions
  install      Enable the hat-glasses control in Store Codex
  uninstall    Remove the Windows Runtime integration
  runtime      Not available on Windows yet
  recover      Not available on Windows yet
  update       Not available on Windows yet
  self-uninstall  Not available on Windows yet

Run incodex <command> --help for details.
";

#[cfg(target_os = "windows")]
pub fn windows_command_help(command: CliCommand) -> &'static str {
    match command {
        CliCommand::Status => {
            "\
Usage:
  incodex status [--json]

Inspect the current user's official Microsoft Store Codex package on Windows.
This command is read-only and discovers the installed location automatically.
"
        }
        CliCommand::Doctor => {
            "\
Usage:
  incodex doctor [--json]

Verify the official Microsoft Store Codex package on Windows and inspect
Incodex-owned sessions without changing either one.
"
        }
        CliCommand::Open => {
            "\
Usage:
  incodex open [--dry-run] [--mask] [--name <text>] [--avatar <local-file>]

Open an isolated Codex window on Windows without patching the official Store
package. The package location is discovered automatically. CODEX_HOME and the
Chromium user-data directory exist only for the isolated session.

Profile masking is only available with --mask. --avatar accepts a local PNG,
JPEG, or WebP file.
"
        }
        CliCommand::Install => {
            "\
Usage:
  incodex install [--yes] [--dry-run]

Enable the Incodex hat-glasses control in the current user's official
Microsoft Store Codex package. Codex must be fully closed. The Store package
is not patched or copied; Incodex registers its separately owned Windows Runtime.

Flags:
  --yes            Skip the confirmation prompt (required when stdin is not a terminal)
  --dry-run, -n    Print the plan and exit
"
        }
        CliCommand::Uninstall => {
            "\
Usage:
  incodex uninstall [--yes] [--dry-run]

Disable and remove the Incodex-owned Windows Runtime integration. Codex must
be fully closed before final removal. The official Store package is unchanged.

Flags:
  --yes            Skip the confirmation prompt (required when stdin is not a terminal)
  --dry-run, -n    Print the plan and exit
"
        }
        CliCommand::Runtime
        | CliCommand::Recover
        | CliCommand::Update
        | CliCommand::SelfUninstall => "This command is not available on Windows yet.\n",
        CliCommand::Menu | CliCommand::Help | CliCommand::Version => WINDOWS_ROOT_HELP,
    }
}

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
  incodex open [--dry-run] [--mask] [--name <text>] [--avatar <local-file>] [--app <path>]

Open an incognito window without patching Codex. Uses an isolated CODEX_HOME
and Chromium user-data-dir. The hat-glasses control and banner still appear
in that window. Closing the window burns that session.

Profile masking is only available with --mask. Without --name, Incodex creates
a temporary name and deterministic avatar. --avatar accepts a local PNG,
JPEG, or WebP file.

Examples:
  incodex open
  incodex open --dry-run
  incodex open --mask --name \"Temporary\" --avatar ./avatar.png
"
        }
        CliCommand::Update => {
            "\
Usage:
  inc update [--dry-run]

Update the CLI through its installation channel. Homebrew installs run
brew update and brew upgrade incodex. Script installs re-run install.sh.
Source checkouts should git pull.

Examples:
  inc update
  inc update --dry-run
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
