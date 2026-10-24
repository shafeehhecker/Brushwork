//! Brushwork core: GUI-independent image model and raster operations.
//!
//! The desktop UI lives in the binary crate (`src/main.rs`, `src/app/`); everything
//! here is plain Rust and covered by unit tests.

pub mod color;
pub mod document;
pub mod fill;
pub mod geom;
pub mod history;
pub mod io;
pub mod paint;
pub mod raster;
pub mod selection;
pub mod shapes;
pub mod text;
pub mod transform;
pub mod view;
