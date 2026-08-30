#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CoordinatorSnapshot {
    pub source_build: u64,
    pub observed_build: Option<u64>,
    pub parent_running: bool,
    pub app_running: bool,
    pub integration_installed: bool,
    pub registered: bool,
    pub grace_expired: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CoordinatorAction {
    Wait,
    ExitNoUpdate,
    ExitCancelled,
    Reinstall { expected_build: u64 },
}

pub fn next_action(snapshot: CoordinatorSnapshot) -> CoordinatorAction {
    if !snapshot.registered {
        return CoordinatorAction::ExitCancelled;
    }
    if snapshot.parent_running || snapshot.app_running {
        return CoordinatorAction::Wait;
    }

    let Some(observed_build) = snapshot.observed_build else {
        return CoordinatorAction::Wait;
    };
    if !snapshot.integration_installed {
        return CoordinatorAction::Reinstall {
            expected_build: observed_build,
        };
    }
    if observed_build != snapshot.source_build {
        return CoordinatorAction::Wait;
    }
    if snapshot.grace_expired {
        CoordinatorAction::ExitNoUpdate
    } else {
        CoordinatorAction::Wait
    }
}
