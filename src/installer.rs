use anyhow::{Context, Result};
use crossterm::{
    execute,
    style::{Color, Print, ResetColor, SetForegroundColor},
};
use std::env;
use std::fs;
use std::io::{stdout, Write};
use std::path::PathBuf;

#[cfg(target_os = "windows")]
use winreg::enums::*;
#[cfg(target_os = "windows")]
use winreg::RegKey;

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
        
        execute!(
            stdout,
            SetForegroundColor(Color::Green),
            Print("\r\n[+] Successfully copied FileDrop!\r\n\r\n"),
            SetForegroundColor(Color::Yellow),
            Print("⚠️  ACTION REQUIRED TO COMPLETE INSTALLATION ⚠️\r\n"),
            ResetColor,
            Print("To run 'filedrop' from any terminal, you must add its folder to your system PATH.\r\n\r\n"),
            Print("How to add it to PATH on Mac/Linux:\r\n"),
            Print("  Add the following line to your ~/.bashrc or ~/.zshrc file:\r\n"),
            SetForegroundColor(Color::Cyan),
            Print(format!("     export PATH=\"$PATH:{}\"\r\n", install_dir.display())),
            ResetColor,
            Print("  Then restart your terminal!\r\n\r\n")
        )?;
    }

    #[cfg(target_os = "windows")]
    {
        execute!(stdout, Print("[*] Adding to system PATH...\r\n"))?;
        
        let hkcu = RegKey::predef(HKEY_CURRENT_USER);
        let env_key = hkcu.open_subkey_with_flags("Environment", KEY_READ | KEY_WRITE)?;
        
        let current_path: String = env_key.get_value("Path").unwrap_or_default();
        let install_path_str = install_dir.display().to_string();
        
        if !current_path.contains(&install_path_str) {
            let new_path = if current_path.is_empty() || current_path.ends_with(';') {
                format!("{}{}", current_path, install_path_str)
            } else {
                format!("{};{}", current_path, install_path_str)
            };
            env_key.set_value("Path", &new_path)?;
            
            unsafe {
                let mut result = 0;
                let env_str: Vec<u16> = "Environment\0".encode_utf16().collect();
                windows_sys::Win32::UI::WindowsAndMessaging::SendMessageTimeoutW(
                    0xffff as isize,
                    windows_sys::Win32::UI::WindowsAndMessaging::WM_SETTINGCHANGE,
                    0,
                    env_str.as_ptr() as isize,
                    windows_sys::Win32::UI::WindowsAndMessaging::SMTO_ABORTIFHUNG,
                    5000,
                    &mut result,
                );
            }
            
            execute!(
                stdout,
                SetForegroundColor(Color::Green),
                Print("\r\n[+] Successfully installed FileDrop and updated PATH!\r\n\r\n"),
                SetForegroundColor(Color::Yellow),
                Print("Note: Your system has been notified of the PATH change. You can now open a new terminal and type 'filedrop'.\r\n\r\n"),
                ResetColor
            )?;
        } else {
            execute!(
                stdout,
                SetForegroundColor(Color::Green),
                Print("\r\n[+] Successfully installed FileDrop! (PATH is already configured)\r\n\r\n"),
                ResetColor
            )?;
        }
    }

    execute!(
        stdout,
        SetForegroundColor(Color::Cyan),
        Print("Quick Guide:\r\n"),
        ResetColor,
        Print("  1. Close this terminal and open a new one.\r\n"),
        Print("  2. Type 'filedrop receive' to receive files into the current folder.\r\n"),
        Print("  3. Type 'filedrop share' to share files from the current folder.\r\n"),
        Print("  4. Type 'filedrop --help' to see all commands.\r\n\r\n"),
        SetForegroundColor(Color::DarkGrey),
        Print("Press Enter to exit...\r\n"),
        ResetColor
    )?;
    stdout.flush()?;

    let mut buf = String::new();
    std::io::stdin().read_line(&mut buf)?;

    Ok(())
}
