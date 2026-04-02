pub mod channels;
pub mod config;
pub mod i18n;
pub mod state;
pub mod utils;

pub use rust_i18n;
rust_i18n::i18n!("locales");
