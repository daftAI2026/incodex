use std::cmp::Ordering;
use std::io::Write;

pub(crate) trait UpdateProgress {
    fn stage(&mut self, message: &str);
    fn stop(&mut self);
}

impl UpdateProgress for crate::spinner::Progress {
    fn stage(&mut self, message: &str) {
        crate::spinner::Progress::stage(self, message);
    }

    fn stop(&mut self) {
        crate::spinner::Progress::stop(self);
    }
}

pub(crate) fn run_update_pipeline<P, W, I, R, T>(
    progress: &mut P,
    stdout: &mut W,
    install: I,
    publish_runtime: R,
) -> Result<(), String>
where
    P: UpdateProgress,
    W: Write,
    I: FnOnce(&mut P) -> Result<(Ordering, String, T), String>,
    R: FnOnce(T) -> Result<String, String>,
{
    progress.stage("Upgrading Incodex");
    let result = (|| {
        let (release_ordering, latest_tag, installed) = install(progress)?;
        progress.stage("Publishing Runtime");
        let installed_version = publish_runtime(installed)?;
        Ok::<_, String>((release_ordering, latest_tag, installed_version))
    })();
    progress.stop();
    let (release_ordering, latest_tag, installed_version) = result?;

    match release_ordering {
        Ordering::Greater => writeln!(
            stdout,
            "🎉 Update ran successfully! Please quit and reopen Codex."
        ),
        Ordering::Equal => writeln!(
            stdout,
            "Already on latest version, {}\nRuntime is synchronized. Fully quit and reopen Codex to reload it.",
            installed_version
        ),
        Ordering::Less => writeln!(
            stdout,
            "Current version {} is newer than latest release {}.\nRuntime is synchronized. Fully quit and reopen Codex to reload it.",
            installed_version,
            latest_tag
        ),
    }
    .map_err(|error| format!("cannot write update result: {error}"))
}

pub(crate) fn run_installer_fallback<P, W, F, C>(
    progress: &mut P,
    warning: &mut W,
    stable_installer: F,
    compatibility_installer: C,
) -> Result<(), String>
where
    P: UpdateProgress,
    W: Write,
    F: FnOnce() -> Result<(), String>,
    C: FnOnce() -> Result<(), String>,
{
    match stable_installer() {
        Ok(()) => Ok(()),
        Err(error) => {
            progress.stop();
            writeln!(warning, "Stable installer did not complete: {error}").map_err(
                |write_error| format!("cannot report the stable installer failure: {write_error}"),
            )?;
            progress.stage("Upgrading Incodex");
            compatibility_installer()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Default)]
    struct RecordingProgress(Vec<String>);

    impl UpdateProgress for RecordingProgress {
        fn stage(&mut self, message: &str) {
            self.0.push(format!("stage:{message}"));
        }

        fn stop(&mut self) {
            self.0.push("stop".to_string());
        }
    }

    #[test]
    fn managed_update_reports_shared_stages_and_success_on_observable_output() {
        let mut progress = RecordingProgress::default();
        let mut stdout = Vec::new();

        run_update_pipeline(
            &mut progress,
            &mut stdout,
            |_| Ok((Ordering::Greater, "v9.9.9".to_string(), ())),
            |_| Ok("9.9.9".to_string()),
        )
        .expect("complete controlled update");

        assert_eq!(
            progress.0,
            [
                "stage:Upgrading Incodex",
                "stage:Publishing Runtime",
                "stop",
            ]
        );
        assert_eq!(
            String::from_utf8(stdout).expect("UTF-8 output"),
            "🎉 Update ran successfully! Please quit and reopen Codex.\n"
        );
    }

    #[test]
    fn compatibility_warning_is_printed_only_while_progress_is_stopped() {
        let mut progress = RecordingProgress::default();
        let mut warning = Vec::new();

        run_installer_fallback(
            &mut progress,
            &mut warning,
            || Err("tagged installer failed".to_string()),
            || Ok(()),
        )
        .expect("compatibility installer succeeds");

        assert_eq!(progress.0, ["stop", "stage:Upgrading Incodex"]);
        assert_eq!(
            String::from_utf8(warning).expect("UTF-8 warning"),
            "Stable installer did not complete: tagged installer failed\n"
        );
    }
}
