use std::io;

fn main() -> io::Result<()> {
    #[cfg(target_os = "windows")]
    {
        let mut res = winres::WindowsResource::new();
        res.set("FileDescription", "FileDrop");
        res.set("ProductName", "FileDrop");
        res.set("OriginalFilename", "filedrop.exe");
        res.set("LegalCopyright", "Copyright (c) Yaswanth Kumar Mallela");
        res.set("CompanyName", "Yaswanth Kumar Mallela");
        res.set("ProductVersion", "0.5.2");
        res.set("FileVersion", "0.5.2");
        res.compile()?;
    }
    Ok(())
}
