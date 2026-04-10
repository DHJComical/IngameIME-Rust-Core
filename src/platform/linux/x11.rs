use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use x11rb::connection::Connection;
use x11rb::protocol::Event;
use x11rb::protocol::xproto::{
    ChangeWindowAttributesAux, ConnectionExt as _, EventMask, KeyButMask, KeyPressEvent,
};
use xkeysym::Keysym;

pub(super) struct X11Backend {
    stop: Arc<AtomicBool>,
    worker: Option<JoinHandle<()>>,
    rx: Receiver<super::backend::EngineEvent>,
}

impl X11Backend {
    pub(super) fn connect(hwnd: isize) -> Option<Self> {
        if std::env::var_os("DISPLAY").is_none() {
            return None;
        }

        let (connection, screen_num) = match x11rb::connect(None) {
            Ok(pair) => pair,
            Err(err) => {
                crate::logger::warn(&format!("X11 backend unavailable: {err}"));
                return None;
            }
        };

        let fallback_root = connection
            .setup()
            .roots
            .get(screen_num)
            .map(|screen| screen.root)
            .unwrap_or(0);
        let window = if hwnd > 0 { hwnd as u32 } else { fallback_root };
        let mask = EventMask::KEY_PRESS | EventMask::FOCUS_CHANGE;
        if let Ok(cookie) = connection
            .change_window_attributes(window, &ChangeWindowAttributesAux::new().event_mask(mask))
        {
            let _ = cookie.check();
        }
        let _ = connection.flush();

        let (tx, rx) = mpsc::channel();
        let stop = Arc::new(AtomicBool::new(false));
        let stop_clone = stop.clone();
        let worker = match thread::Builder::new()
            .name("ingameime-x11-event-loop".to_string())
            .spawn(move || event_loop(connection, tx, stop_clone))
        {
            Ok(handle) => handle,
            Err(err) => {
                crate::logger::warn(&format!("Failed to start X11 event loop: {err}"));
                return None;
            }
        };

        crate::logger::info(&format!(
            "Connected to X11 server (screen={screen_num}, window=0x{window:08x})"
        ));
        Some(Self {
            stop,
            worker: Some(worker),
            rx,
        })
    }

    pub(super) fn name(&self) -> &'static str {
        let _ = self;
        "x11"
    }

    pub(super) fn poll_events(&mut self, out: &mut Vec<super::backend::EngineEvent>) {
        while let Ok(event) = self.rx.try_recv() {
            out.push(event);
        }
    }
}

impl Drop for X11Backend {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

fn event_loop(
    connection: x11rb::rust_connection::RustConnection,
    tx: Sender<super::backend::EngineEvent>,
    stop: Arc<AtomicBool>,
) {
    while !stop.load(Ordering::SeqCst) {
        match connection.poll_for_event() {
            Ok(Some(event)) => handle_event(&connection, event, &tx),
            Ok(None) => thread::sleep(Duration::from_millis(5)),
            Err(err) => {
                crate::logger::warn(&format!("X11 event loop terminated: {err}"));
                break;
            }
        }
    }
}

fn handle_event(
    connection: &x11rb::rust_connection::RustConnection,
    event: Event,
    tx: &Sender<super::backend::EngineEvent>,
) {
    if let Event::KeyPress(ev) = event
        && let Some(text) = translate_keypress(connection, &ev)
    {
        let _ = tx.send(super::backend::EngineEvent::Commit(text));
    }
}

fn translate_keypress(
    connection: &x11rb::rust_connection::RustConnection,
    event: &KeyPressEvent,
) -> Option<String> {
    let blocked_mods = KeyButMask::CONTROL | KeyButMask::MOD1 | KeyButMask::MOD4;
    if event.state.intersects(blocked_mods) {
        return None;
    }

    let reply = connection
        .get_keyboard_mapping(event.detail, 1)
        .ok()?
        .reply()
        .ok()?;
    if reply.keysyms.is_empty() {
        return None;
    }

    let symbols_per_keycode = reply.keysyms_per_keycode as usize;
    if symbols_per_keycode == 0 {
        return None;
    }

    let index = if event.state.contains(KeyButMask::SHIFT) && symbols_per_keycode > 1 {
        1
    } else {
        0
    };

    let keysym = *reply.keysyms.get(index).or_else(|| reply.keysyms.first())?;
    if keysym == 0 {
        return None;
    }

    Some(Keysym::new(keysym).key_char()?.to_string())
}
