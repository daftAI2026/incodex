/*
 * [INPUT]: 依赖隔离 Runtime 夹具和有界 PTY 驱动。
 * [OUTPUT]: 为集成测试暴露 runtime 与 tty 测试模块。
 * [POS]: 通用集成夹具入口；更新场景夹具由 update_support 独立持有。
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */
pub mod runtime;
pub mod tty;
