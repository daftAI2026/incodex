/// Same root help as `src/help.ts` `rootHelp()`. Command help lands in a later step.
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
