//! Windows: embed the app icon (`resources/icon/chixie-rs.ico`) as icon resource 1. Explorer
//! shows it on `chixie-rs.exe`, and gpui loads resource 1 as the window and taskbar icon.

fn main() {
    println!("cargo:rerun-if-changed=chixie-rs.rc");
    println!("cargo:rerun-if-changed=resources/icon/chixie-rs.ico");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        embed_resource::compile("chixie-rs.rc", embed_resource::ParamsIncludeDirs(["resources/icon"]))
            .manifest_optional()
            .unwrap();
    }
}
