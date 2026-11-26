//! Windows: embed the app icon (`resources/icon/penkaku.ico`) as icon resource 1. Explorer
//! shows it on `penkaku.exe`, and gpui loads resource 1 as the window and taskbar icon.

fn main() {
    println!("cargo:rerun-if-changed=penkaku.rc");
    println!("cargo:rerun-if-changed=resources/icon/penkaku.ico");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        embed_resource::compile("penkaku.rc", embed_resource::ParamsIncludeDirs(["resources/icon"]))
            .manifest_optional()
            .unwrap();
    }
}
