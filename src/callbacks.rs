use std::sync::Arc;

use crate::model::InputMode;

// Callback aliases use `Arc` (not `Box`) so a handler can be cloned out of a
// `CallbackStore` while a lock is held, then invoked after the lock is released.
// This avoids running user callbacks under a Mutex (see tsf.rs).
pub type CommitCallback = Arc<dyn Fn(String) + 'static>;

pub struct PreEdit {
    pub text: String,
    pub cursor: usize,
}

pub enum PreEditEvent {
    Begin,
    Update(PreEdit),
    End,
}

pub type PreEditCallback = Arc<dyn Fn(PreEditEvent) + 'static>;

pub struct Candidate {
    pub candidates: Vec<String>,
    pub selected: usize,
}

pub enum CandidateEvent {
    Begin,
    Update(Candidate),
    End,
}

pub type CandidateCallback = Arc<dyn Fn(CandidateEvent) + 'static>;
pub type InputModeCallback = Arc<dyn Fn(InputMode) + 'static>;

#[derive(Default)]
pub struct CallbackStore {
    commit: Option<CommitCallback>,
    preedit: Option<PreEditCallback>,
    candidate: Option<CandidateCallback>,
    input_mode: Option<InputModeCallback>,
}

impl CallbackStore {
    pub fn set_commit(&mut self, callback: CommitCallback) {
        self.commit = Some(callback);
    }

    pub fn set_preedit(&mut self, callback: PreEditCallback) {
        self.preedit = Some(callback);
    }

    pub fn set_candidate(&mut self, callback: CandidateCallback) {
        self.candidate = Some(callback);
    }

    pub fn set_input_mode(&mut self, callback: InputModeCallback) {
        self.input_mode = Some(callback);
    }

    // Getters clone out the `Arc` handle so callers (notably the TSF backend)
    // can drop the `Mutex` guard before invoking the user callback.
    pub fn commit_callback(&self) -> Option<CommitCallback> {
        self.commit.clone()
    }

    pub fn preedit_callback(&self) -> Option<PreEditCallback> {
        self.preedit.clone()
    }

    pub fn candidate_callback(&self) -> Option<CandidateCallback> {
        self.candidate.clone()
    }

    pub fn input_mode_callback(&self) -> Option<InputModeCallback> {
        self.input_mode.clone()
    }

    pub fn emit_commit(&self, text: &str) {
        if let Some(callback) = &self.commit {
            callback(text.to_string());
        }
    }

    pub fn emit_preedit_begin(&self) {
        if let Some(callback) = &self.preedit {
            callback(PreEditEvent::Begin);
        }
    }

    pub fn emit_preedit_update(&self, text: &str, cursor: usize) {
        if let Some(callback) = &self.preedit {
            callback(PreEditEvent::Update(PreEdit {
                text: text.to_string(),
                cursor,
            }));
        }
    }

    pub fn emit_preedit_end(&self) {
        if let Some(callback) = &self.preedit {
            callback(PreEditEvent::End);
        }
    }

    pub fn emit_candidate_begin(&self) {
        if let Some(callback) = &self.candidate {
            callback(CandidateEvent::Begin);
        }
    }

    pub fn emit_candidate_update(&self, candidates: &[String], selected: usize) {
        if let Some(callback) = &self.candidate {
            callback(CandidateEvent::Update(Candidate {
                candidates: candidates.to_vec(),
                selected,
            }));
        }
    }

    pub fn emit_candidate_end(&self) {
        if let Some(callback) = &self.candidate {
            callback(CandidateEvent::End);
        }
    }

    pub fn emit_input_mode(&self, mode: InputMode) {
        if let Some(callback) = &self.input_mode {
            callback(mode);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[test]
    fn commit_callback_getter_clones_and_invokes() {
        let calls = Arc::new(AtomicUsize::new(0));
        let calls_in_cb = Arc::clone(&calls);

        let mut store = CallbackStore::default();
        store.set_commit(Arc::new(move |text: String| {
            assert_eq!(text, "hello");
            calls_in_cb.fetch_add(1, Ordering::SeqCst);
        }));

        // The getter must hand back a usable clone that can be invoked after the
        // store (and, in real usage, its enclosing lock) is no longer borrowed.
        let cb = store
            .commit_callback()
            .expect("commit callback should be set");
        cb("hello".to_string());

        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn getters_return_none_when_unset() {
        let store = CallbackStore::default();
        assert!(store.commit_callback().is_none());
        assert!(store.preedit_callback().is_none());
        assert!(store.candidate_callback().is_none());
        assert!(store.input_mode_callback().is_none());
    }
}
