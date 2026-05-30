use anyhow::{Context, Result};
use crossterm::{
    execute,
    style::{Color, Print, ResetColor, SetForegroundColor},
};
use std::env;
use std::fs;
use std::io::{stdout, Write};
use std::path::PathBuf;

pub fn install_self() -> Result<()> {
    let current_exe = env::current_exe().context("Failed to get current executable path")?;
    let mut stdout = stdout();

    #[cfg(target_os = "windows")]
    let (install_dir, target_exe) = {
        let local_app_data =
            env::var("LOCALAPPDATA").context("LOCALAPPDATA environment variable not found")?;
        let dir = PathBuf::from(local_app_data).join("FileDrop").join("bin");
        let exe = dir.join("filedrop.exe");
        (dir, exe)
    };

    #[cfg(not(target_os = "windows"))]
    let (install_dir, target_exe) = {
        let home = env::var("HOME").context("HOME environment variable not found")?;
        let dir = PathBuf::from(home).join(".local").join("bin");
        let exe = dir.join("filedrop");
        (dir, exe)
    };

    execute!(
        stdout,
        SetForegroundColor(Color::Cyan),
        Print(format!(
            "\r\n[*] Installing to {}...\r\n",
            install_dir.display()
        )),
        ResetColor
    )?;

    fs::create_dir_all(&install_dir)?;
    fs::copy(&current_exe, &target_exe)?;

    #[cfg(not(target_os = "windows"))]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = fs::metadata(&target_exe)?.permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&target_exe, perms)?;
    }

    execute!(
        stdout,
        SetForegroundColor(Color::Green),
        Print("\r\n[+] Successfully copied FileDrop!\r\n\r\n"),
        SetForegroundColor(Color::Yellow),
        Print("⚠️  ACTION REQUIRED TO COMPLETE INSTALLATION ⚠️\r\n"),
        ResetColor,
        Print("To run 'filedrop' from any terminal, you must add its folder to your system PATH.\r\n\r\n")
    )?;

    #[cfg(target_os = "windows")]
    {
        execute!(
            stdout,
            Print("How to add it to PATH on Windows:\r\n"),
            Print("  1. Press the Windows Key, type 'Environment Variables', and press Enter.\r\n"),
            Print("  2. Click 'Environment Variables...' at the bottom.\r\n"),
            Print("  3. Under 'User variables', select 'Path' and click 'Edit...'.\r\n"),
            Print("  4. Click 'New' and paste the following folder path:\r\n"),
            SetForegroundColor(Color::Cyan),
            Print(format!("     {}\r\n", install_dir.display())),
            ResetColor,
            Print("  5. Click OK on all windows, close your terminal, and open a new one!\r\n\r\n")
        )?;
    }

    #[cfg(not(target_os = "windows"))]
    {
        execute!(
            stdout,
            Print("How to add it to PATH on Mac/Linux:\r\n"),
            Print("  Add the following line to your ~/.bashrc or ~/.zshrc file:\r\n"),
            SetForegroundColor(Color::Cyan),
            Print(format!("     export PATH=\"$PATH:{}\"\r\n", install_dir.display())),
            ResetColor,
            Print("  Then restart your terminal!\r\n\r\n")
        )?;
    }

    execute!(
        stdout,
        SetForegroundColor(Color::DarkGrey),
        Print("Press Enter to exit...\r\n"),
        ResetColor
    )?;
    stdout.flush()?;

    let mut buf = String::new();
    std::io::stdin().read_line(&mut buf)?;

    Ok(())
}
