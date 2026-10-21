//! # ScreenFoundry core
//!
//! The screen-printing (silkscreen) prepress engine: turn artwork into one film per screen.
//!
//! * [`image`] – RGBA artwork, 8-bit plates and 1-bit film, with size caps.
//! * [`color`] – sRGB to CIE Lab, for perceptual ink matching.
//! * [`separate`] – palette extraction, spot-colour separation, tone plates.
//! * [`morph`] – exact Euclidean choke and spread of a plate (for underbase and traps).
//! * [`halftone`] – amplitude-modulated screening (round, elliptical, line) at a screen angle.
//! * [`marks`] – registration crosshairs.
//! * [`profile`] – mesh / lines-per-inch / angle sanity checks.
//! * [`job`] – a serialisable job (inks, underbase, trap) and the pipeline that runs it.
//!
//! Every default number here is a practitioner rule of thumb from public sources
//! (`docs/research/silkscreen-landscape.md`); they are inputs the user can change, not facts.
//! Nothing panics on bad input: sizes are capped and every fallible call returns [`Result`].

pub mod color;
pub mod error;
pub mod halftone;
pub mod image;
pub mod job;
pub mod marks;
pub mod morph;
pub mod profile;
pub mod separate;

pub use error::{Result, ScError};
pub use image::{Bitmap1, Gray8, Rgba8};
