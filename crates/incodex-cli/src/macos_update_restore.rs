#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SparkleDownload {
    pub source_build: Option<u64>,
    pub target_build: u64,
    pub bytes: u64,
    pub complete: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CoordinatorPhase {
    Watching,
    AwaitingPatchedExit {
        source_build: u64,
        target_build: u64,
    },
    AwaitingOfficialUpdate {
        source_build: u64,
        target_build: u64,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CoordinatorSnapshot {
    pub phase: CoordinatorPhase,
    pub app_build: u64,
    pub app_running: bool,
    pub integration_installed: bool,
    pub registered: bool,
    pub downloaded_target: Option<u64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CoordinatorAction {
    Wait,
    ExitNoUpdate,
    ExitCancelled,
    PersistIntent {
        source_build: u64,
        target_build: u64,
    },
    RestoreVendorAndLaunch {
        source_build: u64,
        target_build: u64,
    },
    LaunchOfficial {
        target_build: u64,
    },
    Reinstall {
        expected_build: u64,
    },
}

pub fn choose_sparkle_update(
    current_build: u64,
    downloads: &[SparkleDownload],
) -> Option<u64> {
    downloads
        .iter()
        .filter(|download| {
            download.complete
                && download.bytes > 0
                && download.target_build > current_build
                && download
                    .source_build
                    .is_none_or(|source| source == current_build)
        })
        .map(|download| download.target_build)
        .max()
}

pub fn next_action(snapshot: CoordinatorSnapshot) -> CoordinatorAction {
    if !snapshot.registered {
        return CoordinatorAction::ExitCancelled;
    }

    match snapshot.phase {
        CoordinatorPhase::Watching => watching_action(snapshot),
        CoordinatorPhase::AwaitingPatchedExit {
            source_build,
            target_build,
        } => {
            if snapshot.app_running {
                CoordinatorAction::Wait
            } else if snapshot.integration_installed {
                CoordinatorAction::RestoreVendorAndLaunch {
                    source_build,
                    target_build,
                }
            } else {
                CoordinatorAction::LaunchOfficial { target_build }
            }
        }
        CoordinatorPhase::AwaitingOfficialUpdate { target_build, .. } => {
            if snapshot.app_build >= target_build {
                if snapshot.app_running {
                    CoordinatorAction::Wait
                } else {
                    CoordinatorAction::Reinstall {
                        expected_build: snapshot.app_build,
                    }
                }
            } else if snapshot.app_running {
                CoordinatorAction::Wait
            } else {
                CoordinatorAction::LaunchOfficial { target_build }
            }
        }
    }
}

fn watching_action(snapshot: CoordinatorSnapshot) -> CoordinatorAction {
    if let Some(target_build) = snapshot
        .downloaded_target
        .filter(|target| *target > snapshot.app_build)
    {
        return CoordinatorAction::PersistIntent {
            source_build: snapshot.app_build,
            target_build,
        };
    }
    if snapshot.app_running {
        CoordinatorAction::Wait
    } else {
        CoordinatorAction::ExitNoUpdate
    }
}
