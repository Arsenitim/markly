//! Skrib: a lightweight, local-first image annotation
//! utility. The library holds everything that is testable without a GUI;
//! `app` contains the egui front-end.

pub mod app;
pub mod clipboard;
pub mod desktop;
pub mod export;
pub mod geometry;
pub mod icon;
pub mod model;
pub mod render;
pub mod settings;
pub mod store;
pub mod text;
pub mod undo;
