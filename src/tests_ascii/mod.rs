pub mod js;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// Name of a test: it("...") or the function of a #[test].
    Title,
    /// Message text of an assertion.
    Assert,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Finding {
    pub file: String,
    pub line: usize,
    pub kind: Kind,
    /// The offending literal as it is written in the source.
    pub text: String,
}
