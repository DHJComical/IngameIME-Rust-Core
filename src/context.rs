use crate::callbacks::{CandidateCallback, CommitCallback, InputModeCallback, PreEditCallback};
use crate::model::{CandidateConfig, InputMode};

#[cfg(windows)]
enum Backend {
    Imm32(Box<crate::platform::windows::Imm32Backend>),
    Tsf(Box<crate::platform::windows::TsInputContext>),
}

pub struct ImeContext {
    #[cfg(windows)]
    backend: Backend,
    #[cfg(target_os = "linux")]
    backend: crate::platform::linux::LinuxInputContext,
}

impl ImeContext {
    pub fn create(hwnd: isize, api: i32, ui_less: bool) -> Option<Self> {
        #[cfg(windows)]
        {
            if hwnd == 0 {
                return None;
            }

            if api == 0 {
                crate::logger::info("Creating TSF input context");
                if let Some(backend) = crate::platform::windows::TsInputContext::new(hwnd, ui_less)
                {
                    return Some(Self {
                        backend: Backend::Tsf(backend),
                    });
                }
                crate::logger::warn("TSF initialization failed, falling back to IMM32");
            }

            let backend = crate::platform::windows::Imm32Backend::new(hwnd, ui_less)?;
            return Some(Self {
                backend: Backend::Imm32(backend),
            });
        }

        #[cfg(target_os = "linux")]
        {
            let backend = crate::platform::linux::LinuxInputContext::new(hwnd, api, ui_less)?;
            return Some(Self { backend });
        }

        #[cfg(not(any(windows, target_os = "linux")))]
        {
            let _ = (hwnd, api, ui_less);
            None
        }
    }

    pub fn set_activated(&mut self, activated: bool) {
        #[cfg(windows)]
        match &mut self.backend {
            Backend::Imm32(b) => b.set_activated(activated),
            Backend::Tsf(b) => b.set_activated(activated),
        }

        #[cfg(target_os = "linux")]
        self.backend.set_activated(activated);
    }

    pub fn is_activated(&self) -> bool {
        #[cfg(windows)]
        {
            return match &self.backend {
                Backend::Imm32(b) => b.is_activated(),
                Backend::Tsf(b) => b.is_activated(),
            };
        }

        #[cfg(target_os = "linux")]
        {
            self.backend.is_activated()
        }

        #[cfg(not(any(windows, target_os = "linux")))]
        {
            false
        }
    }

    pub fn get_input_mode(&self) -> InputMode {
        #[cfg(windows)]
        {
            return match &self.backend {
                Backend::Imm32(b) => b.input_mode(),
                Backend::Tsf(b) => b.input_mode(),
            };
        }

        #[cfg(target_os = "linux")]
        {
            self.backend.input_mode()
        }

        #[cfg(not(any(windows, target_os = "linux")))]
        {
            InputMode::Unsupported
        }
    }

    pub fn force_alpha_mode(&mut self) {
        #[cfg(windows)]
        match &mut self.backend {
            Backend::Imm32(b) => b.force_alpha_mode(),
            Backend::Tsf(b) => b.force_alpha_mode(),
        }

        #[cfg(target_os = "linux")]
        self.backend.force_alpha_mode();
    }

    pub fn force_native_mode(&mut self) {
        #[cfg(windows)]
        match &mut self.backend {
            Backend::Imm32(b) => b.force_native_mode(),
            Backend::Tsf(b) => b.force_native_mode(),
        }

        #[cfg(target_os = "linux")]
        self.backend.force_native_mode();
    }

    pub fn set_preedit_rect(&mut self, x: i32, y: i32, width: i32, height: i32) {
        #[cfg(windows)]
        match &mut self.backend {
            Backend::Imm32(b) => b.set_preedit_rect(x, y, width, height),
            Backend::Tsf(b) => b.set_preedit_rect(x, y, width, height),
        }

        #[cfg(target_os = "linux")]
        self.backend.set_preedit_rect(x, y, width, height);
    }

    pub fn set_candidate_config(&mut self, config: CandidateConfig) {
        #[cfg(windows)]
        match &mut self.backend {
            Backend::Imm32(b) => b.set_candidate_config(config),
            Backend::Tsf(b) => b.set_candidate_config(config),
        }

        #[cfg(target_os = "linux")]
        self.backend.set_candidate_config(config);
    }

    pub fn candidate_config(&self) -> CandidateConfig {
        #[cfg(windows)]
        {
            return match &self.backend {
                Backend::Imm32(b) => b.candidate_config(),
                Backend::Tsf(b) => b.candidate_config(),
            };
        }

        #[cfg(target_os = "linux")]
        {
            self.backend.candidate_config()
        }

        #[cfg(not(any(windows, target_os = "linux")))]
        {
            CandidateConfig::default()
        }
    }

    pub fn set_commit_callback(&mut self, callback: CommitCallback) {
        #[cfg(windows)]
        match &mut self.backend {
            Backend::Imm32(b) => b.set_commit_callback(callback),
            Backend::Tsf(b) => b.set_commit_callback(callback),
        }

        #[cfg(target_os = "linux")]
        self.backend.set_commit_callback(callback);
    }

    pub fn set_preedit_callback(&mut self, callback: PreEditCallback) {
        #[cfg(windows)]
        match &mut self.backend {
            Backend::Imm32(b) => b.set_preedit_callback(callback),
            Backend::Tsf(b) => b.set_preedit_callback(callback),
        }

        #[cfg(target_os = "linux")]
        self.backend.set_preedit_callback(callback);
    }

    pub fn set_candidate_callback(&mut self, callback: CandidateCallback) {
        #[cfg(windows)]
        match &mut self.backend {
            Backend::Imm32(b) => b.set_candidate_callback(callback),
            Backend::Tsf(b) => b.set_candidate_callback(callback),
        }

        #[cfg(target_os = "linux")]
        self.backend.set_candidate_callback(callback);
    }

    pub fn set_input_mode_callback(&mut self, callback: InputModeCallback) {
        #[cfg(windows)]
        match &mut self.backend {
            Backend::Imm32(b) => b.set_input_mode_callback(callback),
            Backend::Tsf(b) => b.set_input_mode_callback(callback),
        }

        #[cfg(target_os = "linux")]
        self.backend.set_input_mode_callback(callback);
    }
}
