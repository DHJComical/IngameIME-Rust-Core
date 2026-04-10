use crate::callbacks::{
    CallbackStore, CandidateCallback, CommitCallback, InputModeCallback, PreEditCallback,
};
use crate::model::{CandidateConfig, InputMode};

#[allow(dead_code)]
pub(super) enum EngineEvent {
    Commit(String),
    PreEditBegin,
    PreEditUpdate {
        text: String,
        cursor: usize,
    },
    PreEditEnd,
    CandidateBegin,
    CandidateUpdate {
        candidates: Vec<String>,
        selected: usize,
    },
    CandidateEnd,
    InputMode(InputMode),
}

pub struct LinuxInputContext {
    backend: LinuxBackend,
    activated: bool,
    input_mode: InputMode,
    candidate_config: CandidateConfig,
    preedit_rect: (i32, i32, i32, i32),
    callbacks: CallbackStore,
}

impl LinuxInputContext {
    pub fn new(hwnd: isize, api: i32, ui_less: bool) -> Option<Self> {
        let backend = LinuxBackend::connect(hwnd)?;
        crate::logger::info(&format!(
            "Created Linux input context via {} backend (api={api}, ui_less={ui_less})",
            backend.name()
        ));
        Some(Self {
            backend,
            activated: false,
            input_mode: InputMode::Alpha,
            candidate_config: CandidateConfig::default(),
            preedit_rect: (0, 0, 0, 0),
            callbacks: CallbackStore::default(),
        })
    }

    pub fn set_activated(&mut self, activated: bool) {
        self.pump_backend_events();
        self.activated = activated;
        if !activated {
            self.callbacks.emit_preedit_end();
            self.callbacks.emit_candidate_end();
        }
    }

    pub fn is_activated(&self) -> bool {
        self.activated
    }

    pub fn input_mode(&self) -> InputMode {
        self.input_mode
    }

    pub fn force_alpha_mode(&mut self) {
        self.pump_backend_events();
        self.input_mode = InputMode::Alpha;
        self.callbacks.emit_input_mode(self.input_mode);
    }

    pub fn force_native_mode(&mut self) {
        self.pump_backend_events();
        self.input_mode = InputMode::Native;
        self.callbacks.emit_input_mode(self.input_mode);
    }

    pub fn set_preedit_rect(&mut self, x: i32, y: i32, width: i32, height: i32) {
        self.pump_backend_events();
        self.preedit_rect = (x, y, width, height);
    }

    pub fn set_candidate_config(&mut self, config: CandidateConfig) {
        self.pump_backend_events();
        self.candidate_config = config;
    }

    pub fn candidate_config(&self) -> CandidateConfig {
        self.candidate_config.clone()
    }

    pub fn set_commit_callback(&mut self, callback: CommitCallback) {
        self.pump_backend_events();
        self.callbacks.set_commit(callback);
    }

    pub fn set_preedit_callback(&mut self, callback: PreEditCallback) {
        self.pump_backend_events();
        self.callbacks.set_preedit(callback);
    }

    pub fn set_candidate_callback(&mut self, callback: CandidateCallback) {
        self.pump_backend_events();
        self.callbacks.set_candidate(callback);
    }

    pub fn set_input_mode_callback(&mut self, callback: InputModeCallback) {
        self.pump_backend_events();
        self.callbacks.set_input_mode(callback);
        self.callbacks.emit_input_mode(self.input_mode);
    }

    fn pump_backend_events(&mut self) {
        let mut events = Vec::new();
        self.backend.poll_events(&mut events);
        for event in events {
            self.apply_event(event);
        }
    }

    fn apply_event(&mut self, event: EngineEvent) {
        match event {
            EngineEvent::Commit(text) => {
                if self.activated {
                    self.callbacks.emit_commit(&text);
                }
            }
            EngineEvent::PreEditBegin => {
                if self.activated {
                    self.callbacks.emit_preedit_begin();
                }
            }
            EngineEvent::PreEditUpdate { text, cursor } => {
                if self.activated {
                    self.callbacks.emit_preedit_update(&text, cursor);
                }
            }
            EngineEvent::PreEditEnd => {
                if self.activated {
                    self.callbacks.emit_preedit_end();
                }
            }
            EngineEvent::CandidateBegin => {
                if self.activated {
                    self.callbacks.emit_candidate_begin();
                }
            }
            EngineEvent::CandidateUpdate {
                candidates,
                selected,
            } => {
                if self.activated {
                    self.callbacks.emit_candidate_update(&candidates, selected);
                }
            }
            EngineEvent::CandidateEnd => {
                if self.activated {
                    self.callbacks.emit_candidate_end();
                }
            }
            EngineEvent::InputMode(mode) => {
                self.input_mode = mode;
                self.callbacks.emit_input_mode(mode);
            }
        }
    }
}

impl Drop for LinuxInputContext {
    fn drop(&mut self) {
        crate::logger::info(&format!(
            "Dropped Linux input context ({})",
            self.backend.name()
        ));
    }
}

enum LinuxBackend {
    #[cfg(feature = "wayland")]
    Wayland(super::wayland::WaylandBackend),
    #[cfg(feature = "x11")]
    X11(super::x11::X11Backend),
}

impl LinuxBackend {
    fn connect(hwnd: isize) -> Option<Self> {
        #[cfg(not(feature = "x11"))]
        let _ = hwnd;

        #[cfg(feature = "wayland")]
        if let Some(backend) = super::wayland::WaylandBackend::connect() {
            return Some(Self::Wayland(backend));
        }

        #[cfg(feature = "x11")]
        if let Some(backend) = super::x11::X11Backend::connect(hwnd) {
            return Some(Self::X11(backend));
        }

        crate::logger::warn("No Linux IME backend available; enable `wayland` or `x11` feature");
        None
    }

    fn name(&self) -> &'static str {
        match self {
            #[cfg(feature = "wayland")]
            Self::Wayland(backend) => backend.name(),
            #[cfg(feature = "x11")]
            Self::X11(backend) => backend.name(),
            _ => "linux",
        }
    }

    fn poll_events(&mut self, out: &mut Vec<EngineEvent>) {
        match self {
            #[cfg(feature = "wayland")]
            Self::Wayland(backend) => backend.poll_events(out),
            #[cfg(feature = "x11")]
            Self::X11(backend) => backend.poll_events(out),
            _ => {
                let _ = out;
            }
        }
    }
}
