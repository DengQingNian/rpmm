/// 将应用图标嵌入 Windows 命令行程序。参数：无。返回：无，资源编译失败时终止构建。
fn main() {
    println!("cargo:rerun-if-changed=resources/rpmm.rc");
    println!("cargo:rerun-if-changed=src-tauri/icons/icon.ico");
    #[cfg(windows)]
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        embed_resource::compile_for("resources/rpmm.rc", ["rpmm"], embed_resource::NONE)
            .manifest_required()
            .expect("Windows 应用图标资源编译失败");
    }
}
