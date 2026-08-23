// Included by both the binary and build.rs. This is the single declaration of
// the built-in runtime set; build.rs only consumes it for schema generation.
macro_rules! builtin_drivers {
    () => {
        &[
            bombadil_browser::plugin::REGISTRATION,
            #[cfg(feature = "terminal")]
            bombadil_terminal::plugin::REGISTRATION,
        ]
    };
}
