pub(super) struct WaylandBackend {
    _connection: wayland_client::Connection,
}

impl WaylandBackend {
    pub(super) fn connect() -> Option<Self> {
        if std::env::var_os("WAYLAND_DISPLAY").is_none() {
            return None;
        }

        match wayland_client::Connection::connect_to_env() {
            Ok(connection) => {
                crate::logger::info("Connected to Wayland compositor");
                Some(Self {
                    _connection: connection,
                })
            }
            Err(err) => {
                crate::logger::warn(&format!("Wayland backend unavailable: {err}"));
                None
            }
        }
    }

    pub(super) fn name(&self) -> &'static str {
        let _ = self;
        "wayland"
    }

    pub(super) fn poll_events(&mut self, out: &mut Vec<super::backend::EngineEvent>) {
        let _ = (self, out);
    }
}
