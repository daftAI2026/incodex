#![cfg(target_os = "windows")]

use std::fs;
use std::io::Write;

use incodex_cli::windows_activation_capability::WindowsActivationCapability;
use incodex_cli::windows_launch::{
    WindowsActivationEnvironment, WindowsActivationEnvironmentPipe, WindowsLaunchMode,
};

#[test]
fn environment_pipe_refuses_a_client_outside_the_capability_job() {
    let capability = WindowsActivationCapability::create().expect("create capability");
    let pipe = WindowsActivationEnvironmentPipe::create(&capability).expect("create environment pipe");
    let pipe_name = pipe.name().to_string();
    let job_name = capability.job_name().to_string();
    let server = std::thread::spawn(move || {
        pipe.respond_once(
            &job_name,
            "OpenAI.Codex_1.2.3.4_x64__publisher",
            &WindowsActivationEnvironment {
                mode: WindowsLaunchMode::Cdp,
                environment: [("INCODEX_INCOGNITO".to_string(), "1".to_string())]
                    .into_iter()
                    .collect(),
            },
        )
    });
    let mut client = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(&pipe_name)
        .expect("connect environment pipe");
    client
        .write_all(b"environment\n")
        .expect("write environment request");

    let error = server
        .join()
        .expect("join environment server")
        .expect_err("reject client outside activation Job");
    assert!(error.contains("Job"), "{error}");
}
