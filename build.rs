//! Embeds the application icon (and basic version metadata) into `Winbar.exe`
//! as a Windows resource, so the binary shows the Winbar icon in Explorer,
//! Alt-Tab, and anywhere the shell asks the process for its icon. The tray
//! icon loads this same resource (id 1) at runtime.

fn main() {
    println!("cargo:rerun-if-changed=assets/winbar.ico");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let mut res = winresource::WindowsResource::new();
        res.set_icon_with_id("assets/winbar.ico", "1");
        res.set("FileDescription", "Winbar");
        res.set("ProductName", "Winbar");
        res.compile().expect("failed to compile Windows resources");
    }
}
