fn main() {
    println!("cargo:rerun-if-changed=assets/dopad.rc");
    println!("cargo:rerun-if-changed=assets/dopad.ico");

    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows")
        && let Err(error) =
            embed_resource::compile("assets/dopad.rc", embed_resource::NONE).manifest_required()
    {
        panic!("failed to embed Dopad's Windows icon resource: {error}");
    }
}
