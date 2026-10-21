use thiserror::Error;

/// Everything that can go wrong in the separation pipeline. Callers show these to the user.
#[derive(Debug, Error, PartialEq)]
pub enum ScError {
    #[error("image is {width}x{height} but the buffer holds {len} bytes/samples")]
    BadBuffer { width: usize, height: usize, len: usize },
    #[error("image of {width}x{height} exceeds the {max} pixel limit")]
    TooLarge { width: usize, height: usize, max: usize },
    #[error("image has no pixels")]
    Empty,
    #[error("invalid parameter `{name}`: {reason}")]
    Param { name: &'static str, reason: String },
    #[error("plates differ in size")]
    SizeMismatch,
    #[error("registration marks do not fit on the film")]
    MarksDontFit,
}

pub type Result<T> = std::result::Result<T, ScError>;

pub(crate) fn param(name: &'static str, reason: impl Into<String>) -> ScError {
    ScError::Param { name, reason: reason.into() }
}
