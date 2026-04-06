use std::ffi::c_void;
use std::mem::transmute;

use windows::Win32::Foundation::{HANDLE, HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::UI::Input::Ime::{
    CANDIDATEFORM, CANDIDATELIST, CFS_EXCLUDE, CFS_RECT, COMPOSITIONFORM, CPS_CANCEL, GCS_COMPSTR,
    GCS_CURSORPOS, GCS_RESULTSTR, HIMC, IME_CMODE_NATIVE, IME_COMPOSITION_STRING,
    IME_CONVERSION_MODE, IME_SENTENCE_MODE, IMN_CHANGECANDIDATE, IMN_CLOSECANDIDATE,
    IMN_OPENCANDIDATE, IMN_SETCONVERSIONMODE, ISC_SHOWUICANDIDATEWINDOW,
    ISC_SHOWUICOMPOSITIONWINDOW, ImmAssociateContext, ImmCreateContext, ImmDestroyContext,
    ImmGetCandidateListW, ImmGetCompositionStringW, ImmGetConversionStatus, ImmNotifyIME,
    ImmSetCandidateWindow, ImmSetCompositionWindow, ImmSetConversionStatus, ImmSetOpenStatus,
    NI_COMPOSITIONSTR,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CallWindowProcW, DefWindowProcW, GWLP_WNDPROC, GetPropW, RemovePropW, SetPropW,
    SetWindowLongPtrW, WM_IME_CHAR, WM_IME_COMPOSITION, WM_IME_ENDCOMPOSITION, WM_IME_NOTIFY,
    WM_IME_SETCONTEXT, WM_IME_STARTCOMPOSITION, WM_INPUTLANGCHANGE, WNDPROC,
};
use windows::core::w;

use crate::callbacks::{
    CallbackStore, CandidateCallback, CommitCallback, InputModeCallback, PreEditCallback,
};
use crate::logger;
use crate::model::{CandidateConfig, InputMode};

const CONTEXT_PROP: windows::core::PCWSTR = w!("IngameIME_RustCore_Context");

pub struct Imm32Backend {
    hwnd: HWND,
    previous_himc: HIMC,
    himc: HIMC,
    activated: bool,
    ui_less: bool,
    old_wndproc: Option<WNDPROC>,
    preedit_rect: RECT,
    callbacks: CallbackStore,
    candidate_config: CandidateConfig,
    candidate_open: bool,
}

impl Imm32Backend {
    pub fn new(hwnd_raw: isize, ui_less: bool) -> Option<Box<Self>> {
        if hwnd_raw == 0 {
            logger::error("create_input_context_win32 failed: hwnd is zero");
            return None;
        }

        let hwnd = HWND(hwnd_raw as *mut c_void);

        unsafe {
            let himc = ImmCreateContext();
            if himc.is_invalid() {
                logger::error("ImmCreateContext failed");
                return None;
            }

            let previous_himc = ImmAssociateContext(hwnd, HIMC::default());
            let mut backend = Box::new(Self {
                hwnd,
                previous_himc,
                himc,
                activated: false,
                ui_less,
                old_wndproc: None,
                preedit_rect: RECT::default(),
                callbacks: CallbackStore::default(),
                candidate_config: CandidateConfig::default(),
                candidate_open: false,
            });

            let ptr = (&mut *backend) as *mut Self as isize;
            if let Err(err) = SetPropW(hwnd, CONTEXT_PROP, Some(HANDLE(ptr as *mut c_void))) {
                logger::error(&format!("SetPropW failed: {err}"));
                ImmAssociateContext(hwnd, previous_himc);
                let _ = ImmDestroyContext(himc);
                return None;
            }

            let old_ptr = SetWindowLongPtrW(
                hwnd,
                GWLP_WNDPROC,
                window_proc as *const () as usize as isize,
            );
            if old_ptr == 0 {
                logger::warn("SetWindowLongPtrW returned 0 when installing subclass proc");
            }
            backend.old_wndproc = Some(transmute(old_ptr));

            if !ImmSetOpenStatus(himc, true).as_bool() {
                logger::warn("ImmSetOpenStatus returned false");
            }

            logger::info("IMM32 context created");
            Some(backend)
        }
    }

    pub fn set_activated(&mut self, activated: bool) {
        if self.activated == activated {
            return;
        }
        self.activated = activated;

        unsafe {
            if activated {
                ImmAssociateContext(self.hwnd, self.himc);
                self.callbacks.emit_input_mode(self.input_mode());
            } else {
                let _ = ImmNotifyIME(self.himc, NI_COMPOSITIONSTR, CPS_CANCEL, 0);
                ImmAssociateContext(self.hwnd, HIMC::default());
                self.callbacks.emit_preedit_end();
                self.callbacks.emit_candidate_end();
                self.candidate_open = false;
            }
        }
    }

    pub fn is_activated(&self) -> bool {
        self.activated
    }

    pub fn input_mode(&self) -> InputMode {
        unsafe {
            let mut conversion = IME_CONVERSION_MODE(0);
            if !ImmGetConversionStatus(self.himc, Some(&mut conversion as *mut _), None).as_bool() {
                return InputMode::Unsupported;
            }

            if conversion.contains(IME_CMODE_NATIVE) {
                InputMode::Native
            } else {
                InputMode::Alpha
            }
        }
    }

    pub fn force_alpha_mode(&mut self) {
        unsafe {
            let mut conversion = IME_CONVERSION_MODE(0);
            let mut sentence = IME_SENTENCE_MODE(0);
            if !ImmGetConversionStatus(
                self.himc,
                Some(&mut conversion as *mut _),
                Some(&mut sentence as *mut _),
            )
            .as_bool()
            {
                logger::warn("force_alpha_mode: ImmGetConversionStatus failed");
                return;
            }

            if !conversion.contains(IME_CMODE_NATIVE) {
                return;
            }

            let next = IME_CONVERSION_MODE(conversion.0 & !IME_CMODE_NATIVE.0);
            if ImmSetConversionStatus(self.himc, next, sentence).as_bool() {
                self.callbacks.emit_input_mode(InputMode::Alpha);
            }
        }
    }

    pub fn force_native_mode(&mut self) {
        unsafe {
            let mut conversion = IME_CONVERSION_MODE(0);
            let mut sentence = IME_SENTENCE_MODE(0);
            if !ImmGetConversionStatus(
                self.himc,
                Some(&mut conversion as *mut _),
                Some(&mut sentence as *mut _),
            )
            .as_bool()
            {
                logger::warn("force_native_mode: ImmGetConversionStatus failed");
                return;
            }

            if conversion.contains(IME_CMODE_NATIVE) {
                return;
            }

            let next = IME_CONVERSION_MODE(conversion.0 | IME_CMODE_NATIVE.0);
            if ImmSetConversionStatus(self.himc, next, sentence).as_bool() {
                self.callbacks.emit_input_mode(InputMode::Native);
            }
        }
    }

    pub fn set_preedit_rect(&mut self, x: i32, y: i32, width: i32, height: i32) {
        let width = width.max(1);
        let height = height.max(1);
        let rect = RECT {
            left: x,
            top: y,
            right: x + width,
            bottom: y + height,
        };

        if rect == self.preedit_rect {
            return;
        }

        self.preedit_rect = rect;

        unsafe {
            let mut candidate = CANDIDATEFORM::default();
            candidate.dwStyle = CFS_EXCLUDE;
            candidate.ptCurrentPos.x = x;
            candidate.ptCurrentPos.y = y;
            candidate.rcArea = rect;
            let _ = ImmSetCandidateWindow(self.himc, &candidate);

            let mut composition = COMPOSITIONFORM::default();
            composition.dwStyle = CFS_RECT;
            composition.ptCurrentPos.x = x;
            composition.ptCurrentPos.y = y;
            composition.rcArea = rect;
            let _ = ImmSetCompositionWindow(self.himc, &composition);
        }
    }

    pub fn set_candidate_config(&mut self, config: CandidateConfig) {
        self.candidate_config = config;
    }

    pub fn candidate_config(&self) -> CandidateConfig {
        self.candidate_config.clone()
    }

    pub fn set_commit_callback(&mut self, callback: CommitCallback) {
        self.callbacks.set_commit(callback);
    }

    pub fn set_preedit_callback(&mut self, callback: PreEditCallback) {
        self.callbacks.set_preedit(callback);
    }

    pub fn set_candidate_callback(&mut self, callback: CandidateCallback) {
        self.callbacks.set_candidate(callback);
    }

    pub fn set_input_mode_callback(&mut self, callback: InputModeCallback) {
        self.callbacks.set_input_mode(callback);
    }

    fn handle_window_message(&mut self, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
        match msg {
            WM_INPUTLANGCHANGE => {
                self.callbacks.emit_input_mode(self.input_mode());
                self.forward(msg, wparam, lparam)
            }
            WM_IME_SETCONTEXT => {
                let mut flags = lparam.0;
                flags &= !(ISC_SHOWUICOMPOSITIONWINDOW as isize);
                if self.ui_less {
                    flags &= !(ISC_SHOWUICANDIDATEWINDOW as isize);
                }
                self.forward(msg, wparam, LPARAM(flags))
            }
            WM_IME_STARTCOMPOSITION => {
                self.callbacks.emit_preedit_begin();
                if self.ui_less {
                    self.refresh_candidates();
                }
                LRESULT(0)
            }
            WM_IME_COMPOSITION => {
                let composition_flags = IME_COMPOSITION_STRING(lparam.0 as u32);
                if composition_flags.contains(GCS_COMPSTR)
                    || composition_flags.contains(GCS_CURSORPOS)
                {
                    self.refresh_preedit();
                }
                if composition_flags.contains(GCS_RESULTSTR) {
                    self.emit_commit_text();
                }
                if self.ui_less {
                    self.refresh_candidates();
                }
                LRESULT(0)
            }
            WM_IME_ENDCOMPOSITION => {
                self.callbacks.emit_preedit_end();
                LRESULT(0)
            }
            WM_IME_NOTIFY => match wparam.0 as u32 {
                IMN_OPENCANDIDATE => {
                    if self.ui_less && !self.candidate_open {
                        self.candidate_open = true;
                        self.callbacks.emit_candidate_begin();
                        self.refresh_candidates();
                    }
                    LRESULT(0)
                }
                IMN_CHANGECANDIDATE => {
                    if self.ui_less {
                        self.refresh_candidates();
                    }
                    LRESULT(0)
                }
                IMN_CLOSECANDIDATE => {
                    if self.ui_less {
                        self.candidate_open = false;
                        self.callbacks.emit_candidate_end();
                    }
                    LRESULT(0)
                }
                IMN_SETCONVERSIONMODE => {
                    self.callbacks.emit_input_mode(self.input_mode());
                    LRESULT(0)
                }
                _ => self.forward(msg, wparam, lparam),
            },
            WM_IME_CHAR => LRESULT(0),
            _ => self.forward(msg, wparam, lparam),
        }
    }

    fn forward(&self, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
        if let Some(old) = self.old_wndproc {
            unsafe { CallWindowProcW(old, self.hwnd, msg, wparam, lparam) }
        } else {
            unsafe { DefWindowProcW(self.hwnd, msg, wparam, lparam) }
        }
    }

    fn refresh_preedit(&mut self) {
        let Some(text) = self.read_composition_string(GCS_COMPSTR) else {
            return;
        };

        let cursor = unsafe {
            ImmGetCompositionStringW(self.himc, GCS_CURSORPOS, None, 0)
                .max(0)
                .min(i32::MAX) as usize
        };
        self.callbacks.emit_preedit_update(&text, cursor);
    }

    fn emit_commit_text(&mut self) {
        if let Some(text) = self.read_composition_string(GCS_RESULTSTR) {
            self.callbacks.emit_commit(&text);
        }
    }

    fn read_composition_string(&self, kind: IME_COMPOSITION_STRING) -> Option<String> {
        unsafe {
            let byte_len = ImmGetCompositionStringW(self.himc, kind, None, 0);
            if byte_len <= 0 {
                return None;
            }

            let mut buffer = vec![0u8; byte_len as usize];
            let copied = ImmGetCompositionStringW(
                self.himc,
                kind,
                Some(buffer.as_mut_ptr() as *mut c_void),
                byte_len as u32,
            );
            if copied <= 0 {
                return None;
            }

            Some(utf16_bytes_to_string(&buffer))
        }
    }

    fn refresh_candidates(&mut self) {
        let (mut candidates, selected) = self.read_candidate_page();
        if candidates.len() > self.candidate_config.max_candidates {
            candidates.truncate(self.candidate_config.max_candidates);
        }
        self.callbacks.emit_candidate_update(&candidates, selected);
    }

    fn read_candidate_page(&self) -> (Vec<String>, usize) {
        unsafe {
            let required_bytes = ImmGetCandidateListW(self.himc, 0, None, 0);
            if required_bytes == 0 {
                return (Vec::new(), 0);
            }

            let mut bytes = vec![0u8; required_bytes as usize];
            let copied = ImmGetCandidateListW(
                self.himc,
                0,
                Some(bytes.as_mut_ptr() as *mut CANDIDATELIST),
                required_bytes,
            );
            if copied == 0 || copied as usize > bytes.len() {
                return (Vec::new(), 0);
            }

            let header = std::ptr::read_unaligned(bytes.as_ptr() as *const CANDIDATELIST);
            let count = header.dwCount as usize;
            if count == 0 {
                return (Vec::new(), 0);
            }

            let page_start = (header.dwPageStart as usize).min(count);
            let page_size = if header.dwPageSize == 0 {
                count.saturating_sub(page_start)
            } else {
                (header.dwPageSize as usize).min(count.saturating_sub(page_start))
            };

            let offset_table_base = 6 * size_of::<u32>();
            if bytes.len() < offset_table_base + count * size_of::<u32>() {
                return (Vec::new(), 0);
            }

            let mut offsets = Vec::with_capacity(count);
            for i in 0..count {
                let at = offset_table_base + i * 4;
                let offset =
                    u32::from_le_bytes([bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]])
                        as usize;
                offsets.push(offset);
            }

            let mut out = Vec::with_capacity(page_size);
            for idx in page_start..(page_start + page_size) {
                let start = offsets[idx];
                let end = if idx + 1 < count {
                    offsets[idx + 1]
                } else {
                    copied as usize
                };
                if start >= end || end > bytes.len() {
                    continue;
                }

                let mut text = utf16_bytes_to_string(&bytes[start..end]);
                text = text.trim().to_string();
                if !text.is_empty() {
                    out.push(text);
                }
            }

            let selected = (header.dwSelection as usize).saturating_sub(page_start);
            (out, selected)
        }
    }
}

impl Drop for Imm32Backend {
    fn drop(&mut self) {
        unsafe {
            self.set_activated(false);
            let _ = RemovePropW(self.hwnd, CONTEXT_PROP);

            if let Some(proc) = self.old_wndproc {
                let raw: isize = transmute(proc);
                let _ = SetWindowLongPtrW(self.hwnd, GWLP_WNDPROC, raw);
            }

            ImmAssociateContext(self.hwnd, self.previous_himc);
            if !ImmDestroyContext(self.himc).as_bool() {
                logger::warn("ImmDestroyContext returned false");
            }
        }
    }
}

unsafe extern "system" fn window_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    let handle = unsafe { GetPropW(hwnd, CONTEXT_PROP) };
    if handle.0.is_null() {
        return unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) };
    }

    let backend = unsafe { &mut *(handle.0 as *mut Imm32Backend) };
    backend.handle_window_message(msg, wparam, lparam)
}

fn utf16_bytes_to_string(bytes: &[u8]) -> String {
    let mut units = Vec::with_capacity(bytes.len() / 2);
    for pair in bytes.chunks_exact(2) {
        units.push(u16::from_le_bytes([pair[0], pair[1]]));
    }
    while units.last().copied() == Some(0) {
        let _ = units.pop();
    }
    String::from_utf16_lossy(&units)
}
