//! Headless tests for the UI in `ui/`. The library only exposes the compiled
//! components; the tests are in `tests.rs`.

slint::include_modules!();

#[cfg(test)]
mod tests;
