/*
 * [INPUT]: 依赖 CLI 命令分派、错误文案与终端状态。
 * [OUTPUT]: 提供原生二进制入口与稳定退出码。
 * [POS]: 产品进程边界；平台逻辑留在库模块。
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */
use std::io::IsTerminal;

fn main() {
    if let Err(failure) = incodex_cli::run_with_exit_code(std::env::args().skip(1)) {
        if !failure.message().is_empty() {
            eprintln!(
                "{}",
                incodex_core::format_error(
                    failure.message(),
                    Some(std::io::stderr().is_terminal())
                )
            );
        }
        std::process::exit(failure.exit_code());
    }
}
