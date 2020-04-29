#[cfg(windows)]
fn main() {
    use std::env;

    let gstreamer_dir = env::var_os("GSTREAMER_1_0_ROOT_X86_64")
        .and_then(|x| x.into_string().ok())
        .unwrap_or_else(|| r#"C:\gstreamer\1.0\x86_64\"#.to_string());

    println!(r"cargo:rustc-link-search=native={}\lib", gstreamer_dir);
}

#[cfg(not(windows))]
fn main() {
}
