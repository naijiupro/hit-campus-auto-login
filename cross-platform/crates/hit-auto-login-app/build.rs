fn main() {
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }

    println!("cargo:rerun-if-changed=resources/windows.rc");
    println!("cargo:rerun-if-changed=resources/windows.manifest");
    println!("cargo:rerun-if-changed=resources/wifi.ico");
    embed_resource::compile("resources/windows.rc", embed_resource::NONE)
        .manifest_required()
        .expect("Windows manifest resource compilation failed");
}
