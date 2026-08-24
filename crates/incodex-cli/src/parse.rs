/**
 * [INPUT]: 接收 native CLI 的命令名与未解释的 flag 令牌
 * [OUTPUT]: 对外提供 ParsedCli，供各命令实现消费已验证的命令边界
 * [POS]: incodex-cli 的唯一命令语言入口；只在 open 命令暴露 profile mask 选项
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CliCommand {
    Menu,
    Help,
    Version,
    Install,
    Uninstall,
    Status,
    Doctor,
    Recover,
    Runtime,
    Open,
    Update,
    SelfUninstall,
}

impl CliCommand {
    fn parse(raw: &str) -> Result<Self, String> {
        match raw {
            "help" => Ok(Self::Help),
            "version" => Ok(Self::Version),
            "install" => Ok(Self::Install),
            "uninstall" => Ok(Self::Uninstall),
            "status" => Ok(Self::Status),
            "doctor" => Ok(Self::Doctor),
            "recover" => Ok(Self::Recover),
            "runtime" => Ok(Self::Runtime),
            "open" => Ok(Self::Open),
            "update" => Ok(Self::Update),
            "self-uninstall" => Ok(Self::SelfUninstall),
            "menu" => Err(format!("unknown command: {raw}\n  incodex --help")),
            _ => Err(format!("unknown command: {raw}\n  incodex --help")),
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Menu => "menu",
            Self::Help => "help",
            Self::Version => "version",
            Self::Install => "install",
            Self::Uninstall => "uninstall",
            Self::Status => "status",
            Self::Doctor => "doctor",
            Self::Recover => "recover",
            Self::Runtime => "runtime",
            Self::Open => "open",
            Self::Update => "update",
            Self::SelfUninstall => "self-uninstall",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedCli {
    pub command: CliCommand,
    pub help: bool,
    pub clone: bool,
    pub live: bool,
    pub yes: bool,
    pub dry_run: bool,
    pub json: bool,
    pub deep: bool,
    pub restore_app: bool,
    pub app: Option<String>,
    pub transaction: Option<String>,
    pub mask: bool,
    pub name: Option<String>,
    pub avatar: Option<String>,
}

const SWITCH_FLAGS: &[&str] = &[
    "--help",
    "-h",
    "--clone",
    "--live",
    "--yes",
    "--confirm-live",
    "--dry-run",
    "-n",
    "--json",
    "--deep",
    "--restore-app",
    "--mask",
];

const VALUE_FLAGS: &[&str] = &["--app", "--transaction", "--name", "--avatar"];

pub fn parse_cli(args: &[String]) -> Result<ParsedCli, String> {
    let raw = args.first().map(String::as_str);
    let flags = if args.is_empty() { &[][..] } else { &args[1..] };
    let command = parse_command(raw)?;
    reject_unknown_args(flags)?;
    let help =
        command == CliCommand::Help || flags.iter().any(|flag| flag == "--help" || flag == "-h");
    let clone = flags.iter().any(|flag| flag == "--clone");
    let live_flag = flags.iter().any(|flag| flag == "--live");
    let yes = flags
        .iter()
        .any(|flag| flag == "--yes" || flag == "--confirm-live");
    let dry_run = flags.iter().any(|flag| flag == "--dry-run" || flag == "-n");
    let json = flags.iter().any(|flag| flag == "--json");
    let deep = flags.iter().any(|flag| flag == "--deep");
    let restore_app = flags.iter().any(|flag| flag == "--restore-app");
    let app = value_after(flags, "--app")?;
    let transaction = value_after(flags, "--transaction")?;
    let mask = flags.iter().any(|flag| flag == "--mask");
    let name = value_after(flags, "--name")?;
    let avatar = value_after(flags, "--avatar")?;

    if clone && live_flag {
        return Err("--clone and --live cannot be used together".to_string());
    }
    if clone && app.is_some() {
        return Err("--clone and --app cannot be used together".to_string());
    }
    if deep && command != CliCommand::Doctor {
        return Err("--deep is only valid for doctor\n  incodex doctor --deep".to_string());
    }
    if command == CliCommand::Recover && !help && transaction.is_none() {
        return Err(
            "recover requires --transaction <id>\n  incodex recover --transaction <id>".to_string(),
        );
    }

    if command != CliCommand::Open {
        if mask {
            return Err("--mask is only valid for open\n  incodex open --mask".to_string());
        }
        if name.is_some() {
            return Err("--name is only valid for open\n  incodex open --mask --name <value>".to_string());
        }
        if avatar.is_some() {
            return Err(
                "--avatar is only valid for open\n  incodex open --mask --avatar <path>".to_string(),
            );
        }
    }
    if command == CliCommand::Open && !mask {
        if name.is_some() {
            return Err(
                "--name requires --mask\n  incodex open --mask --name <value>".to_string(),
            );
        }
        if avatar.is_some() {
            return Err(
                "--avatar requires --mask\n  incodex open --mask --avatar <path>".to_string(),
            );
        }
    }

    let official = !clone && app.is_none();
    let live = matches!(command, CliCommand::Install | CliCommand::Uninstall) && official;
    Ok(ParsedCli {
        command,
        help,
        clone,
        live,
        yes,
        dry_run,
        json,
        deep,
        restore_app,
        app,
        transaction,
        mask,
        name,
        avatar,
    })
}

fn parse_command(raw: Option<&str>) -> Result<CliCommand, String> {
    match raw {
        None | Some("") => Ok(CliCommand::Menu),
        Some("-h") | Some("--help") | Some("help") => Ok(CliCommand::Help),
        Some("-V") | Some("--version") | Some("version") => Ok(CliCommand::Version),
        Some(value) => CliCommand::parse(value),
    }
}

fn reject_unknown_args(flags: &[String]) -> Result<(), String> {
    let mut i = 0;
    while i < flags.len() {
        let arg = flags[i].as_str();
        if VALUE_FLAGS.contains(&arg) {
            i += 2;
            continue;
        }
        if SWITCH_FLAGS.contains(&arg) {
            i += 1;
            continue;
        }
        if arg.starts_with('-') {
            return Err(format!("unknown flag: {arg}\n  incodex --help"));
        }
        return Err(format!("unexpected argument: {arg}\n  incodex --help"));
    }
    Ok(())
}

fn value_after(flags: &[String], name: &str) -> Result<Option<String>, String> {
    let index = match flags.iter().position(|flag| flag == name) {
        Some(index) => index,
        None => return Ok(None),
    };
    let value = flags.get(index + 1);
    match value {
        None => Err(format!("{name} requires a path, not another flag")),
        Some(value) if value.starts_with('-') => {
            Err(format!("{name} requires a path, not another flag"))
        }
        Some(value) => Ok(Some(value.clone())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(items: &[&str]) -> Vec<String> {
        items.iter().map(|item| (*item).to_string()).collect()
    }

    #[test]
    fn no_args_open_the_menu() {
        let parsed = parse_cli(&args(&[])).unwrap();
        assert_eq!(parsed.command, CliCommand::Menu);
        assert!(!parsed.help);
    }

    #[test]
    fn unknown_flags_fail_closed() {
        assert!(parse_cli(&args(&["status", "--please"]))
            .unwrap_err()
            .starts_with("unknown flag: --please"));
        assert_eq!(
            parse_cli(&args(&["status", "--app"])).unwrap_err(),
            "--app requires a path, not another flag"
        );
    }

    #[test]
    fn profile_mask_options_are_parsed_only_for_open() {
        let parsed = parse_cli(&args(&[
            "open",
            "--mask",
            "--name",
            "Temporary",
            "--avatar",
            "/tmp/avatar.png",
        ]))
        .unwrap();

        assert!(parsed.mask);
        assert_eq!(parsed.name.as_deref(), Some("Temporary"));
        assert_eq!(parsed.avatar.as_deref(), Some("/tmp/avatar.png"));
    }

    #[test]
    fn profile_name_and_avatar_require_mask() {
        for flag in ["--name", "--avatar"] {
            let result = parse_cli(&args(&["open", flag, "value"]));
            let value_name = if flag == "--avatar" { "path" } else { "value" };
            assert_eq!(
                result.unwrap_err(),
                format!("{flag} requires --mask\n  incodex open --mask {flag} <{value_name}>")
            );
        }
    }

    #[test]
    fn profile_mask_flags_are_rejected_by_other_commands() {
        let result = parse_cli(&args(&["status", "--mask"]));
        assert_eq!(
            result.unwrap_err(),
            "--mask is only valid for open\n  incodex open --mask"
        );
    }
}
