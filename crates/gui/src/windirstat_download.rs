//! Optional inbox-PowerShell downloader. No policy bypass, script downloads or silent install.
use std::os::windows::fs::OpenOptionsExt;
use std::os::windows::process::CommandExt;
use std::{
    io,
    path::PathBuf,
    process::{Command, Stdio},
    sync::atomic::{AtomicBool, Ordering},
    time::Duration,
};
#[derive(Debug)]
pub struct Package {
    pub path: PathBuf,
    pub version: String,
    pub sha256: String,
    pub cached: bool,
}
pub fn directory() -> PathBuf {
    super::windirstat::state_file().with_file_name("downloads")
}
pub fn available() -> bool {
    directory().join("WinDirStat-x64.msi").is_file()
}
const SCRIPT: &str = r#"
$ErrorActionPreference='Stop'
$ProgressPreference='SilentlyContinue'
[Console]::OutputEncoding=New-Object System.Text.UTF8Encoding($false)
$source='https://api.github.com/repos/windirstat/windirstat/releases/latest'
$root=$env:HYPERSEARCH_WDS_ROOT
New-Item -ItemType Directory -Force -Path $root | Out-Null
$file=Join-Path $root 'WinDirStat-x64.msi'
$record=Join-Path $root 'verified.json'
$partial=Join-Path $root 'WinDirStat-x64.partial'
if ((Test-Path -LiteralPath $file) -and (Test-Path -LiteralPath $record)) {
  try {
    $saved=Get-Content -LiteralPath $record -Raw -Encoding UTF8 | ConvertFrom-Json
    $hash=(Get-FileHash -LiteralPath $file -Algorithm SHA256).Hash.ToLowerInvariant()
    if (($saved.source -eq $source) -and ($saved.sha256 -match '^[0-9a-f]{64}$') -and ($hash -eq $saved.sha256)) {
      $saved.cache_hit=$true
      $saved | ConvertTo-Json -Compress
      exit 0
    }
  } catch { }
}
try {
  $release=Invoke-RestMethod -Uri $source -Headers @{Accept='application/vnd.github+json'}
  $asset=@($release.assets | Where-Object { $_.name -ceq 'WinDirStat-x64.msi' })[0]
  if (($null -eq $asset) -or ($asset.digest -notmatch '^sha256:([0-9a-fA-F]{64})$')) { throw 'Official SHA-256 unavailable; use the official website.' }
  $expected=$matches[1].ToLowerInvariant()
  $url=[Uri]$asset.browser_download_url
  if (($url.Scheme -ne 'https') -or ($url.Host -ne 'github.com') -or (-not $url.AbsolutePath.StartsWith('/windirstat/windirstat/releases/download/'))) { throw 'Unexpected release download origin.' }
  $cached=$false
  if (Test-Path -LiteralPath $file) { $cached=((Get-FileHash -LiteralPath $file -Algorithm SHA256).Hash.ToLowerInvariant() -eq $expected) }
  if (-not $cached) {
    Invoke-WebRequest -Uri $url.AbsoluteUri -UseBasicParsing -OutFile $partial
    $actual=(Get-FileHash -LiteralPath $partial -Algorithm SHA256).Hash.ToLowerInvariant()
    if ($actual -ne $expected) { throw 'SHA-256 mismatch; installer will not be opened.' }
    Move-Item -LiteralPath $partial -Destination $file -Force
    try { Set-Content -LiteralPath $file -Stream Zone.Identifier -Value "[ZoneTransfer]`r`nZoneId=3`r`nHostUrl=$($url.AbsoluteUri)" } catch { }
  }
  $result=[ordered]@{source=$source;path=$file;version=$release.tag_name;sha256=$expected;cache_hit=$cached}
  $result | ConvertTo-Json -Compress | Set-Content -LiteralPath $record -Encoding UTF8
  $result | ConvertTo-Json -Compress
} finally {
  if (Test-Path -LiteralPath $partial) { Remove-Item -LiteralPath $partial -Force }
}
"#;
fn parse(text: &str, expected: &std::path::Path) -> io::Result<Package> {
    let value: serde_json::Value = serde_json::from_str(text.trim()).map_err(io::Error::other)?;
    if value["source"].as_str()
        != Some("https://api.github.com/repos/windirstat/windirstat/releases/latest")
    {
        return Err(io::Error::other("unverified download origin"));
    }
    let sha = value["sha256"]
        .as_str()
        .filter(|s| s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit()))
        .ok_or_else(|| io::Error::other("invalid SHA-256"))?;
    let path = PathBuf::from(
        value["path"]
            .as_str()
            .ok_or_else(|| io::Error::other("missing installer path"))?,
    );
    if path != expected {
        return Err(io::Error::other("unexpected installer path"));
    }
    Ok(Package {
        path,
        version: value["version"].as_str().unwrap_or("unknown").into(),
        sha256: sha.into(),
        cached: value["cache_hit"].as_bool().unwrap_or(false),
    })
}
pub fn download(cancel: &AtomicBool) -> io::Result<Package> {
    download_in(&directory(), cancel)
}
fn download_in(root: &std::path::Path, cancel: &AtomicBool) -> io::Result<Package> {
    if cancel.load(Ordering::Relaxed) {
        return Err(io::Error::other("Загрузка отменена"));
    }
    std::fs::create_dir_all(root)?;
    // Share mode zero serializes downloaders across multiple HyperSearch instances.
    let _lock = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .share_mode(0)
        .open(root.join("download.lock"))?;
    let error_path = root.join("download.stderr");
    let stderr = std::fs::File::create(&error_path)?;
    let executable = std::env::var_os("SystemRoot")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("C:\\Windows"))
        .join("System32/WindowsPowerShell/v1.0/powershell.exe");
    let mut child = Command::new(executable)
        .args(["-NoProfile", "-NonInteractive", "-Command", SCRIPT])
        .env("HYPERSEARCH_WDS_ROOT", root)
        .creation_flags(0x08000000)
        .stdout(Stdio::piped())
        .stderr(Stdio::from(stderr))
        .spawn()?;
    loop {
        if cancel.load(Ordering::Relaxed) {
            let _ = child.kill();
            let _ = child.wait();
            let _ = std::fs::remove_file(root.join("WinDirStat-x64.partial"));
            return Err(io::Error::other("Загрузка отменена"));
        }
        if child.try_wait()?.is_some() {
            break;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    let output = child.wait_with_output()?;
    if !output.status.success() {
        return Err(io::Error::other(
            "Не удалось загрузить или проверить WinDirStat. Откройте официальный сайт.",
        ));
    }
    let package = parse(
        &String::from_utf8_lossy(&output.stdout),
        &root.join("WinDirStat-x64.msi"),
    )?;
    let _ = std::fs::write(
        super::windirstat::state_file().with_file_name("windirstat.download"),
        package.path.to_string_lossy().as_bytes(),
    );
    let _ = std::fs::remove_file(error_path);
    Ok(package)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn reject_unverified_download_records_and_cancel_before_network() {
        let path = PathBuf::from("C:\\cache\\WinDirStat-x64.msi");
        assert!(
            parse(
            r#"{"source":"https://api.github.com/repos/windirstat/windirstat/releases/latest","path":"C:\\cache\\WinDirStat-x64.msi","sha256":"bad"}"#,
                &path
            )
            .is_err()
        );
        let json = serde_json::json!({"source":"https://api.github.com/repos/windirstat/windirstat/releases/latest","path":path,"sha256":"a".repeat(64),"version":"test","cache_hit":true});
        assert!(parse(&json.to_string(), &path).unwrap().cached);
        assert!(parse(&json.to_string(), std::path::Path::new("C:\\other.msi")).is_err());
        assert!(
            download(&AtomicBool::new(true))
                .unwrap_err()
                .to_string()
                .contains("отменена")
        );
        let blocked = std::env::temp_dir().join(format!("hs-wds-blocked-{}", std::process::id()));
        std::fs::write(&blocked, b"fixture, not a directory").unwrap();
        assert!(download_in(&blocked, &AtomicBool::new(false)).is_err());
        std::fs::remove_file(blocked).unwrap();
    }
}
