#[cfg(not(target_os = "windows"))]
pub mod canonical;
#[cfg(not(target_os = "windows"))]
pub mod paths;
pub mod print;
#[cfg(not(target_os = "windows"))]
pub mod session;
#[cfg(not(target_os = "windows"))]
pub mod target;
#[cfg(target_os = "windows")]
pub mod windows_path;
#[cfg(target_os = "windows")]
pub mod windows_session;

#[cfg(not(target_os = "windows"))]
pub use canonical::{
    canonical_path, inspect_target, is_official_app, recheck_target, CanonicalTarget,
};
#[cfg(not(target_os = "windows"))]
pub use paths::{default_app, user_root, DEFAULT_APP};
pub use print::{format_error, format_kv, format_ok, format_step, format_warn};
#[cfg(not(target_os = "windows"))]
pub use target::target_id;
