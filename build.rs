// Embeds the application icon and version information into the Windows executable.
fn main() {
    println!("cargo:rerun-if-changed=assets/brushwork.ico");
    #[cfg(windows)]
    {
        if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
            let mut res = winresource::WindowsResource::new();
            res.set_icon("assets/brushwork.ico");
            res.set("ProductName", "Brushwork");
            res.set("FileDescription", "Brushwork image editor");
            res.set("LegalCopyright", "MIT licence");
            if let Err(e) = res.compile() {
                // Missing resource compiler: the program still works, just without an embedded icon.
                println!("cargo:warning=could not embed the Windows icon: {e}");
            }
        }
    }
}
