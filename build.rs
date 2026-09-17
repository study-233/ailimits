// build.rs — embedding Windows resources (the icon).
fn main() {
    // Re-run (and re-embed) whenever the icon changes.
    println!("cargo:rerun-if-changed=assets/icon.ico");
    #[cfg(target_os = "windows")]
    {
        let mut res = winres::WindowsResource::new();
        res.set_icon("assets/icon.ico");
        res.set("ProductName", "QuotaBar");
        res.set("FileDescription", "QuotaBar - Codex quota for your taskbar");
        res.set("CompanyName", "study-233");
        res.set(
            "LegalCopyright",
            "Original code (C) 2026 napxlexn; modifications by study-233",
        );
        if let Err(e) = res.compile() {
            // A missing icon must not break the build.
            println!("cargo:warning=winres failed: {e}");
        }
    }
}
