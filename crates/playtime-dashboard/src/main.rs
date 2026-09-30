/// The UI compiled from `ui/*.slint` by `build.rs`. Slint's generated code uses `unwrap`, which the workspace lints
/// otherwise forbid; the allowance is limited to this module.
mod ui {
    #![allow(clippy::unwrap_used, clippy::expect_used)]
    slint::include_modules!();
}

fn main() -> Result<(), slint::PlatformError> {
    use slint::ComponentHandle;
    ui::AppWindow::new()?.run()
}
