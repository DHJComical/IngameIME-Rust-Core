use crate::model::InputMode;

pub type CommitCallback = Box<dyn Fn(String) + 'static>;

pub struct PreEdit {
    pub text: String,
    pub cursor: usize,
}

pub enum PreEditEvent {
    Begin,
    Update(PreEdit),
    End,
}

pub type PreEditCallback = Box<dyn Fn(PreEditEvent) + 'static>;

pub struct Candidate {
    pub candidates: Vec<String>,
    pub selected: usize,
}

pub enum CandidateEvent {
    Begin,
    Update(Candidate),
    End,
}

pub type CandidateCallback = Box<dyn Fn(CandidateEvent) + 'static>;
pub type InputModeCallback = Box<dyn Fn(InputMode) + 'static>;

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
