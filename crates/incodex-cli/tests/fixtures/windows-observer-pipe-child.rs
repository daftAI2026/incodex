/*
 * [INPUT]: 依赖夹具可执行文件同目录下的 ready/release 标记。
 * [OUTPUT]: 提供无窗口且保持存活的独立子进程，用于真实句柄继承回归。
 * [POS]: 仅测试的 GUI 进程，不访问宿主、会话、安装登记或用户数据。
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */
#![windows_subsystem = "windows"]

fn main() {
    let executable = std::env::current_exe().unwrap();
    let directory = executable.parent().unwrap();
    std::fs::write(directory.join("ready"), b"ready").unwrap();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
    while !directory.join("release").exists() && std::time::Instant::now() < deadline {
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
}
