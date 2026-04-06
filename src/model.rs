#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InputMode {
    Alpha,
    Native,
    Unsupported,
}

#[derive(Clone, Debug)]
pub struct CandidateConfig {
    pub max_candidates: usize,
}

impl Default for CandidateConfig {
    fn default() -> Self {
        Self { max_candidates: 9 }
    }
}
