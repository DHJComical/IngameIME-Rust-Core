use std::collections::HashMap;
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use async_io::Timer;
use futures_lite::{StreamExt, future};
use zbus::zvariant::{OwnedObjectPath, OwnedValue};
use zbus::{Connection, Message, proxy};

use super::backend::EngineEvent;
use crate::model::InputMode;

const IBUS_SERVICE: &str = "org.freedesktop.IBus";
const IBUS_CLIENT_NAME: &str = "IngameIME";
const IBUS_CAP_PREEDIT_TEXT: u32 = 1 << 0;
const IBUS_CAP_AUXILIARY_TEXT: u32 = 1 << 1;
const IBUS_CAP_LOOKUP_TABLE: u32 = 1 << 2;
const IBUS_CAP_FOCUS: u32 = 1 << 3;
const IBUS_CAPABILITIES: u32 =
    IBUS_CAP_PREEDIT_TEXT | IBUS_CAP_AUXILIARY_TEXT | IBUS_CAP_LOOKUP_TABLE | IBUS_CAP_FOCUS;
const IBUS_RELEASE_MASK: u32 = 1 << 30;

#[proxy(
    interface = "org.freedesktop.IBus",
    default_service = "org.freedesktop.IBus",
    default_path = "/org/freedesktop/IBus"
)]
trait IBus {
    fn create_input_context(&self, client_name: &str) -> zbus::Result<OwnedObjectPath>;
}

#[proxy(interface = "org.freedesktop.IBus.InputContext")]
trait IBusInputContext {
    fn process_key_event(&self, keyval: u32, keycode: u32, state: u32) -> zbus::Result<bool>;
    fn set_cursor_location(&self, x: i32, y: i32, w: i32, h: i32) -> zbus::Result<()>;
    fn focus_in(&self) -> zbus::Result<()>;
    fn focus_out(&self) -> zbus::Result<()>;
    fn reset(&self) -> zbus::Result<()>;
    fn enable(&self) -> zbus::Result<()>;
    fn disable(&self) -> zbus::Result<()>;
    fn set_capabilities(&self, caps: u32) -> zbus::Result<()>;
}

pub(super) struct IbusBackend {
    tx: Sender<WorkerCommand>,
    rx: Receiver<EngineEvent>,
    worker: Option<JoinHandle<()>>,
}

impl IbusBackend {
    pub(super) fn connect() -> Option<Self> {
        let (tx, command_rx) = mpsc::channel();
        let (event_tx, rx) = mpsc::channel();
        let (ready_tx, ready_rx) = mpsc::sync_channel(1);

        let worker = match thread::Builder::new()
            .name("ingameime-ibus".to_string())
            .spawn(move || worker_main(command_rx, event_tx, ready_tx))
        {
            Ok(worker) => worker,
            Err(err) => {
                crate::logger::warn(&format!("Failed to start IBus worker: {err}"));
                return None;
            }
        };

        match ready_rx.recv_timeout(Duration::from_secs(3)) {
            Ok(Ok(())) => {
                crate::logger::info("Connected to IBus input context");
                Some(Self {
                    tx,
                    rx,
                    worker: Some(worker),
                })
            }
            Ok(Err(err)) => {
                crate::logger::warn(&format!("IBus backend unavailable: {err}"));
                let _ = worker.join();
                None
            }
            Err(err) => {
                crate::logger::warn(&format!("Timed out waiting for IBus worker: {err}"));
                let _ = worker.join();
                None
            }
        }
    }

    pub(super) fn name(&self) -> &'static str {
        let _ = self;
        "ibus"
    }

    pub(super) fn poll_events(&mut self, out: &mut Vec<EngineEvent>) {
        while let Ok(event) = self.rx.try_recv() {
            out.push(event);
        }
    }

    pub(super) fn set_activated(&mut self, activated: bool) {
        let _ = self.tx.send(WorkerCommand::SetActivated(activated));
    }

    pub(super) fn set_preedit_rect(&mut self, x: i32, y: i32, width: i32, height: i32) {
        let _ = self
            .tx
            .send(WorkerCommand::SetCursorRect(x, y, width, height));
    }

    pub(super) fn force_alpha_mode(&mut self) {
        let _ = self.tx.send(WorkerCommand::ForceAlphaMode);
    }

    pub(super) fn force_native_mode(&mut self) {
        let _ = self.tx.send(WorkerCommand::ForceNativeMode);
    }

    pub(super) fn process_key_event(
        &mut self,
        keyval: u32,
        keycode: u32,
        state: u32,
        is_release: bool,
    ) -> bool {
        let (reply_tx, reply_rx) = mpsc::channel();
        let command = WorkerCommand::ProcessKeyEvent {
            keyval,
            keycode,
            state,
            is_release,
            reply: reply_tx,
        };
        if self.tx.send(command).is_err() {
            return false;
        }

        reply_rx
            .recv_timeout(Duration::from_millis(250))
            .unwrap_or(false)
    }
}

impl Drop for IbusBackend {
    fn drop(&mut self) {
        let _ = self.tx.send(WorkerCommand::Stop);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

enum WorkerCommand {
    SetActivated(bool),
    SetCursorRect(i32, i32, i32, i32),
    ForceAlphaMode,
    ForceNativeMode,
    ProcessKeyEvent {
        keyval: u32,
        keycode: u32,
        state: u32,
        is_release: bool,
        reply: Sender<bool>,
    },
    Stop,
}

struct WorkerState {
    proxy: IBusInputContextProxy<'static>,
    focused: bool,
    cursor_rect: (i32, i32, i32, i32),
    preedit_visible: bool,
    preedit_text: String,
    preedit_cursor: usize,
    candidate_visible: bool,
    candidate_list: Vec<String>,
    candidate_selected: usize,
}

impl WorkerState {
    async fn connect() -> Result<Self, String> {
        let connection = Connection::session()
            .await
            .map_err(|err| format!("failed to open session bus: {err}"))?;
        let bus = IBusProxy::new(&connection)
            .await
            .map_err(|err| format!("failed to create IBus bus proxy: {err}"))?;
        let path = bus
            .create_input_context(IBUS_CLIENT_NAME)
            .await
            .map_err(|err| format!("CreateInputContext failed: {err}"))?;
        let proxy = IBusInputContextProxy::builder(&connection)
            .destination(IBUS_SERVICE)
            .map_err(|err| format!("failed to set IBus destination: {err}"))?
            .path(path.clone())
            .map_err(|err| format!("failed to set input context path: {err}"))?
            .build()
            .await
            .map_err(|err| format!("failed to create input context proxy: {err}"))?;

        if let Err(err) = proxy.set_capabilities(IBUS_CAPABILITIES).await {
            crate::logger::warn(&format!("IBus SetCapabilities failed: {err}"));
        }

        Ok(Self {
            proxy,
            focused: false,
            cursor_rect: (0, 0, 0, 0),
            preedit_visible: false,
            preedit_text: String::new(),
            preedit_cursor: 0,
            candidate_visible: false,
            candidate_list: Vec::new(),
            candidate_selected: 0,
        })
    }

    async fn handle_command(
        &mut self,
        command: WorkerCommand,
        event_tx: &Sender<EngineEvent>,
    ) -> bool {
        match command {
            WorkerCommand::SetActivated(activated) => {
                if activated {
                    if !self.focused {
                        if let Err(err) = self.proxy.focus_in().await {
                            crate::logger::warn(&format!("IBus FocusIn failed: {err}"));
                        } else {
                            self.focused = true;
                        }
                    }
                    let (x, y, w, h) = self.cursor_rect;
                    if let Err(err) = self.proxy.set_cursor_location(x, y, w, h).await {
                        crate::logger::warn(&format!("IBus SetCursorLocation failed: {err}"));
                    }
                } else {
                    if let Err(err) = self.proxy.reset().await {
                        crate::logger::warn(&format!("IBus Reset failed: {err}"));
                    }
                    if self.focused {
                        if let Err(err) = self.proxy.focus_out().await {
                            crate::logger::warn(&format!("IBus FocusOut failed: {err}"));
                        }
                        self.focused = false;
                    }
                    self.hide_preedit(event_tx);
                    self.hide_candidates(event_tx);
                }
            }
            WorkerCommand::SetCursorRect(x, y, w, h) => {
                self.cursor_rect = (x, y, w, h);
                if self.focused
                    && let Err(err) = self.proxy.set_cursor_location(x, y, w, h).await
                {
                    crate::logger::warn(&format!("IBus SetCursorLocation failed: {err}"));
                }
            }
            WorkerCommand::ForceAlphaMode => {
                if let Err(err) = self.proxy.disable().await {
                    crate::logger::warn(&format!("IBus Disable failed: {err}"));
                } else {
                    let _ = event_tx.send(EngineEvent::InputMode(InputMode::Alpha));
                }
            }
            WorkerCommand::ForceNativeMode => {
                if let Err(err) = self.proxy.enable().await {
                    crate::logger::warn(&format!("IBus Enable failed: {err}"));
                } else {
                    let _ = event_tx.send(EngineEvent::InputMode(InputMode::Native));
                }
            }
            WorkerCommand::ProcessKeyEvent {
                keyval,
                keycode,
                state,
                is_release,
                reply,
            } => {
                let ibus_state = if is_release {
                    state | IBUS_RELEASE_MASK
                } else {
                    state & !IBUS_RELEASE_MASK
                };
                let handled = match self
                    .proxy
                    .process_key_event(keyval, keycode, ibus_state)
                    .await
                {
                    Ok(handled) => handled,
                    Err(err) => {
                        crate::logger::warn(&format!("IBus ProcessKeyEvent failed: {err}"));
                        false
                    }
                };
                let _ = reply.send(handled);
            }
            WorkerCommand::Stop => return false,
        }

        true
    }

    fn handle_signal(&mut self, message: Message, event_tx: &Sender<EngineEvent>) {
        let header = message.header();
        let Some(member) = header.member() else {
            return;
        };

        match member.as_str() {
            "CommitText" => {
                if let Ok((value,)) = message.body().deserialize::<(OwnedValue,)>() {
                    if let Some(text) = parse_ibus_text(value) {
                        let _ = event_tx.send(EngineEvent::Commit(text));
                    }
                }
            }
            "UpdatePreeditText" => {
                if let Ok((value, cursor_pos, visible, _mode)) =
                    message.body().deserialize::<(OwnedValue, u32, bool, u32)>()
                {
                    self.update_preedit(value, cursor_pos, visible, event_tx);
                } else if let Ok((value, cursor_pos, visible)) =
                    message.body().deserialize::<(OwnedValue, u32, bool)>()
                {
                    self.update_preedit(value, cursor_pos, visible, event_tx);
                }
            }
            "ShowPreeditText" => self.show_preedit(event_tx),
            "HidePreeditText" => self.hide_preedit(event_tx),
            "UpdateLookupTable" => {
                if let Ok((value, visible)) = message.body().deserialize::<(OwnedValue, bool)>()
                    && let Some((candidates, selected)) = parse_lookup_table(value)
                {
                    self.update_candidates(candidates, selected, visible, event_tx);
                }
            }
            "ShowLookupTable" => self.show_candidates(event_tx),
            "HideLookupTable" => self.hide_candidates(event_tx),
            "Enabled" => {
                let _ = event_tx.send(EngineEvent::InputMode(InputMode::Native));
            }
            "Disabled" => {
                let _ = event_tx.send(EngineEvent::InputMode(InputMode::Alpha));
            }
            _ => {}
        }
    }

    fn update_preedit(
        &mut self,
        value: OwnedValue,
        cursor_pos: u32,
        visible: bool,
        event_tx: &Sender<EngineEvent>,
    ) {
        self.preedit_text = parse_ibus_text(value).unwrap_or_default();
        self.preedit_cursor = cursor_pos as usize;

        if visible {
            if !self.preedit_visible {
                self.preedit_visible = true;
                let _ = event_tx.send(EngineEvent::PreEditBegin);
            }
            let _ = event_tx.send(EngineEvent::PreEditUpdate {
                text: self.preedit_text.clone(),
                cursor: self.preedit_cursor,
            });
        } else {
            self.hide_preedit(event_tx);
        }
    }

    fn show_preedit(&mut self, event_tx: &Sender<EngineEvent>) {
        if !self.preedit_visible {
            self.preedit_visible = true;
            let _ = event_tx.send(EngineEvent::PreEditBegin);
        }
        let _ = event_tx.send(EngineEvent::PreEditUpdate {
            text: self.preedit_text.clone(),
            cursor: self.preedit_cursor,
        });
    }

    fn hide_preedit(&mut self, event_tx: &Sender<EngineEvent>) {
        if self.preedit_visible {
            self.preedit_visible = false;
            let _ = event_tx.send(EngineEvent::PreEditEnd);
        }
    }

    fn update_candidates(
        &mut self,
        candidates: Vec<String>,
        selected: usize,
        visible: bool,
        event_tx: &Sender<EngineEvent>,
    ) {
        self.candidate_list = candidates;
        self.candidate_selected = selected.min(self.candidate_list.len().saturating_sub(1));

        if visible {
            if !self.candidate_visible {
                self.candidate_visible = true;
                let _ = event_tx.send(EngineEvent::CandidateBegin);
            }
            let _ = event_tx.send(EngineEvent::CandidateUpdate {
                candidates: self.candidate_list.clone(),
                selected: self.candidate_selected,
            });
        } else {
            self.hide_candidates(event_tx);
        }
    }

    fn show_candidates(&mut self, event_tx: &Sender<EngineEvent>) {
        if !self.candidate_visible {
            self.candidate_visible = true;
            let _ = event_tx.send(EngineEvent::CandidateBegin);
        }
        let _ = event_tx.send(EngineEvent::CandidateUpdate {
            candidates: self.candidate_list.clone(),
            selected: self.candidate_selected,
        });
    }

    fn hide_candidates(&mut self, event_tx: &Sender<EngineEvent>) {
        if self.candidate_visible {
            self.candidate_visible = false;
            let _ = event_tx.send(EngineEvent::CandidateEnd);
        }
    }
}

fn worker_main(
    command_rx: Receiver<WorkerCommand>,
    event_tx: Sender<EngineEvent>,
    ready_tx: mpsc::SyncSender<Result<(), String>>,
) {
    async_io::block_on(async move {
        let mut state = match WorkerState::connect().await {
            Ok(state) => state,
            Err(err) => {
                let _ = ready_tx.send(Err(err));
                return;
            }
        };

        let mut signals = match state.proxy.inner().receive_all_signals().await {
            Ok(signals) => signals,
            Err(err) => {
                let _ = ready_tx.send(Err(format!("failed to subscribe to IBus signals: {err}")));
                return;
            }
        };

        let _ = ready_tx.send(Ok(()));

        loop {
            loop {
                match command_rx.try_recv() {
                    Ok(command) => {
                        if !state.handle_command(command, &event_tx).await {
                            return;
                        }
                    }
                    Err(mpsc::TryRecvError::Empty) => break,
                    Err(mpsc::TryRecvError::Disconnected) => return,
                }
            }

            let next_signal = future::race(async { signals.next().await }, async {
                Timer::after(Duration::from_millis(10)).await;
                None
            })
            .await;

            if let Some(message) = next_signal {
                state.handle_signal(message, &event_tx);
            }
        }
    });
}

fn parse_ibus_text(value: OwnedValue) -> Option<String> {
    let (_name, _props, text, _attrs): (String, HashMap<String, OwnedValue>, String, OwnedValue) =
        value.try_into().ok()?;
    Some(text)
}

fn parse_lookup_table(value: OwnedValue) -> Option<(Vec<String>, usize)> {
    let (
        _name,
        _props,
        _page_size,
        cursor_pos,
        _cursor_visible,
        _round,
        _orientation,
        candidates,
        _labels,
    ): (
        String,
        HashMap<String, OwnedValue>,
        u32,
        u32,
        bool,
        bool,
        i32,
        Vec<OwnedValue>,
        Vec<OwnedValue>,
    ) = value.try_into().ok()?;

    let candidates = candidates
        .into_iter()
        .filter_map(parse_ibus_text)
        .collect::<Vec<_>>();
    let selected = cursor_pos as usize;
    Some((candidates, selected))
}
