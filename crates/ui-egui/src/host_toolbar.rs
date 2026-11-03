//! Optional Android assistant state, integrated into the native ribbon tabs.
#[derive(Default)]
pub struct HostToolbar {
    pub action: Option<&'static str>,
    pub assistant_open: bool,
    pub busy: bool,
    pub status: String,
}

/// Help › Check for Updates: true when the host checks its own releases instead (a build with
/// the assistant must not be replaced by the original project's, which has none).
pub fn check_for_updates(app: &mut crate::SheetApp) -> bool {
    match &mut app.services.host_toolbar {
        Some(host) => {
            host.action = Some("check_update");
            true
        }
        None => false,
    }
}
