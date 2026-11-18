//! Embeds the application icon in the executable.
//!
//! Without this the program wears the toolchain's default icon in Explorer, in
//! the taskbar and in the Start Menu entry that points at it. With it, one file
//! carries the identity everywhere, and the shortcut needs no side file.
//!
//! Windows only: the resource format is Windows', and nothing else reads it.

fn main() {
    println!("cargo:rerun-if-changed=assets/app-icon.ico");
    println!("cargo:rerun-if-changed=icon.rc");

    #[cfg(windows)]
    {
        // `compile` answers with a `CompilationResult`, not a `Result`: a
        // toolchain without the resource compiler is reported as
        // `NotAttempted`, and only a real failure is `Failed`. The icon is
        // cosmetic, so even that is a warning rather than a broken build.
        match embed_resource::compile("icon.rc", embed_resource::NONE).manifest_optional() {
            Ok(()) => {}
            Err(failure) => {
                println!("cargo:warning=the application icon was not embedded: {failure:?}")
            }
        }
    }
}
