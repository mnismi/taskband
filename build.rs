//! Embeds the application icon (and basic version metadata) into `Taskband.exe`
//! as a Windows resource, so the binary shows the Taskband icon in Explorer,
//! Alt-Tab, and anywhere the shell asks the process for its icon. The tray
//! icon loads this same resource (id 1) at runtime.

fn main() {
    println!("cargo:rerun-if-changed=assets/taskband.ico");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let mut res = winresource::WindowsResource::new();
        res.set_icon_with_id("assets/taskband.ico", "1");
        res.set("FileDescription", "Taskband");
        res.set("ProductName", "Taskband");
        res.compile().expect("failed to compile Windows resources");
    }
}
