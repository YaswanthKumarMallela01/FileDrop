use anyhow::{Context, Result};
use crossterm::{
    execute,
    style::{Color, Print, ResetColor, SetForegroundColor},
};
use std::env;
use std::fs;
use std::io::{stdout, Write};
use std::path::PathBuf;
use std::process::Command;

pub fn install_self() -> Result<()> {
    let current_exe = env::current_exe().context("Failed to get current executable path")?;
    let mut stdout = stdout();

    #[cfg(target_os = "windows")]
    {
        // Target: %LOCALAPPDATA%\FileDrop\bin\filedrop.exe
        let local_app_data =
            env::var("LOCALAPPDATA").context("LOCALAPPDATA environment variable not found")?;
        let install_dir = PathBuf::from(local_app_data).join("FileDrop").join("bin");

        execute!(
            stdout,
            Print(format!(
                "\r\n[*] Installing to {}...\r\n",
                install_dir.display()
            ))
        )?;

        fs::create_dir_all(&install_dir)?;
        let target_exe = install_dir.join("filedrop.exe");

        fs::copy(&current_exe, &target_exe)?;

        execute!(stdout, Print("[*] Adding to system PATH...\r\n"))?;

        // Add to User PATH via PowerShell
        let script = format!(
            "$oldPath = [Environment]::GetEnvironmentVariable('Path', 'User'); \
             if ($oldPath -notmatch [regex]::Escape('{}')) {{ \
                 $newPath = $oldPath + ';{}'; \
                 [Environment]::SetEnvironmentVariable('Path', $newPath, 'User'); \
             }}",
            install_dir.display(),
            install_dir.display()
        );

        let status = Command::new("powershell")
            .arg("-NoProfile")
            .arg("-Command")
            .arg(&script)
            .status()?;

        if !status.success() {
            anyhow::bail!("Failed to modify PATH via PowerShell.");
        }
    }

    #[cfg(not(target_os = "windows"))]
    {
        let home = env::var("HOME").context("HOME environment variable not found")?;
        let install_dir = PathBuf::from(home).join(".local").join("bin");

        execute!(
            stdout,
            Print(format!(
                "\r\n[*] Installing to {}...\r\n",
                install_dir.display()
            ))
        )?;

        fs::create_dir_all(&install_dir)?;
        let target_exe = install_dir.join("filedrop");

        fs::copy(&current_exe, &target_exe)?;

        // Set executable permissions
        use std::os::unix::fs::PermissionsExt;
        let mut perms = fs::metadata(&target_exe)?.permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&target_exe, perms)?;

        execute!(
            stdout,
            Print("[*] Make sure ~/.local/bin is in your PATH.\r\n")
        )?;
    }

    execute!(
        stdout,
        SetForegroundColor(Color::Green),
        Print("\r\n[+] Successfully installed FileDrop!\r\n\r\n"),
        SetForegroundColor(Color::Cyan),
        Print("Quick Guide:\r\n"),
        ResetColor,
        Print("  1. Close this terminal and open a new one.\r\n"),
        Print("  2. Type 'filedrop receive' to receive files into the current folder.\r\n"),
        Print("  3. Type 'filedrop share' to share files from the current folder.\r\n"),
        Print("  4. Type 'filedrop --help' to see all commands.\r\n\r\n"),
        SetForegroundColor(Color::Yellow),
        Print("Press Enter to continue...\r\n"),
        ResetColor
    )?;
    stdout.flush()?;

    let mut buf = String::new();
    std::io::stdin().read_line(&mut buf)?;

    Ok(())
}
