//! # ScreenFoundry core
//!
//! The screen-printing (silkscreen) prepress engine: turn artwork into one film per screen.
//!
//! * [`image`] – RGBA artwork, 8-bit plates and 1-bit film, with size caps.
//! * [`color`] – sRGB to CIE Lab, for perceptual ink matching.
//! * [`separate`] – palette extraction, spot-colour separation, tone plates.
//! * [`simulated`] – simulated-process separation: full colour from a few inks as halftone dots.
//! * [`morph`] – exact Euclidean choke and spread of a plate (for underbase and traps).
//! * [`halftone`] – amplitude-modulated screening (round, elliptical, line) at a screen angle.
//! * [`marks`] – registration crosshairs.
//! * [`dtf`] – direct-to-film files: mirrored colour art and a white layer.
//! * [`export`] – 1-bit PNG and TIFF film files with their resolution.
//! * [`jobsheet`] – the PDF job sheet that travels with the films.
//! * [`profile`] – mesh / lines-per-inch / angle sanity checks.
//! * [`index`] – index colour: a fixed palette of inks, one per square pixel, by error diffusion.
//! * [`inklib`] – a generic ink library and nearest-ink matching.
//! * [`job`] – a serialisable job (inks, underbase, trap) and the pipeline that runs it.
//!
//! Every default number here is a practitioner rule of thumb from public sources
//! (`docs/research/silkscreen-landscape.md`); they are inputs the user can change, not facts.
//! Nothing panics on bad input: sizes are capped and every fallible call returns [`Result`].

pub mod color;
pub mod dtf;
pub mod error;
pub mod export;
pub mod halftone;
pub mod image;
pub mod index;
pub mod inklib;
pub mod job;
pub mod jobsheet;
pub mod marks;
pub mod morph;
mod pdfout;
pub mod profile;
pub mod separate;
pub mod simulated;

pub use error::{Result, ScError};
pub use image::{Bitmap1, Gray8, Rgba8};
