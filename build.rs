fn main() {
    println!("cargo:rerun-if-changed=resources/teeproxy.rc");
    println!("cargo:rerun-if-changed=resources/teeproxy.manifest");
    println!("cargo:rerun-if-changed=resources/teeproxy.ico");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        embed_resource::compile("resources/teeproxy.rc", embed_resource::NONE)
            .manifest_required()
            .expect("Windows resources require the Windows SDK or MinGW resource compiler");
    }
}
