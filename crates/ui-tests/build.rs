// The app's own build script: theme generation and UI compilation.
#[path = "../app/build.rs"]
mod app_build;

fn main() {
    // slint-build names the translation domain after the package; the UI's
    // strings live in caior.po, the app package's domain.
    // SAFETY: the build script is single threaded and nothing has read the
    // environment yet.
    unsafe {
        std::env::set_var("CARGO_PKG_NAME", "caior");
        // Element names for the testing backend's ElementHandle queries.
        std::env::set_var("SLINT_EMIT_DEBUG_INFO", "1");
    }
    app_build::main();
}
