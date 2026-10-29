fn main() {
    // Android 15+ devices can use 16 KB memory pages, and Play requires native libraries aligned
    // to them. NDK r27 still links for 4 KB by default.
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("android") {
        println!("cargo:rustc-link-arg=-Wl,-z,max-page-size=16384");
    }
    tauri_build::build()
}
