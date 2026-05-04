pub(super) struct WaylandBackend {
    _connection: wayland_client::Connection,
}

impl WaylandBackend {
    pub(super) fn connect() -> Option<Self> {
        let _ = std::env::var_os("WAYLAND_DISPLAY");
        crate::logger::warn(
            "Wayland fallback backend is currently disabled; waiting for IBus or another native backend",
        );
        None
    }

    pub(super) fn name(&self) -> &'static str {
        let _ = self;
        "wayland"
    }

    pub(super) fn poll_events(&mut self, out: &mut Vec<super::backend::EngineEvent>) {
        let _ = (self, out);
    }

    pub(super) fn set_activated(&mut self, activated: bool) {
        let _ = (self, activated);
    }

    pub(super) fn set_preedit_rect(&mut self, x: i32, y: i32, width: i32, height: i32) {
        let _ = (self, x, y, width, height);
    }

    pub(super) fn force_alpha_mode(&mut self) {
        let _ = self;
    }

    pub(super) fn force_native_mode(&mut self) {
        let _ = self;
    }

    pub(super) fn process_key_event(
        &mut self,
        keyval: u32,
        keycode: u32,
        state: u32,
        is_release: bool,
    ) -> bool {
        let _ = (self, keyval, keycode, state, is_release);
        false
    }
}
