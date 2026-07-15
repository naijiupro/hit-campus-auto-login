use std::{fs, io, path::PathBuf};

use directories::BaseDirs;
use hit_auto_login_core::Configuration;

pub fn config_path() -> io::Result<PathBuf> {
    let base = BaseDirs::new().ok_or_else(|| io::Error::other("无法确定用户配置目录"))?;
    #[cfg(target_os = "windows")]
    let path = base.config_dir().join("HITAutoLogin").join("config.json");
    #[cfg(not(target_os = "windows"))]
    let path = base.config_dir().join("hit-auto-login").join("config.json");
    Ok(path)
}

pub fn load() -> io::Result<Configuration> {
    let path = config_path()?;
    if !path.exists() {
        return Ok(Configuration::default());
    }
    let bytes = fs::read(path)?;
    serde_json::from_slice(&bytes)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "配置文件格式无效"))
}

pub fn save(configuration: &Configuration) -> io::Result<()> {
    let path = config_path()?;
    let directory = path
        .parent()
        .ok_or_else(|| io::Error::other("配置目录无效"))?;
    fs::create_dir_all(directory)?;
    let temporary = path.with_extension("json.tmp");
    let data =
        serde_json::to_vec_pretty(configuration).map_err(|_| io::Error::other("无法序列化配置"))?;
    fs::write(&temporary, data)?;
    set_private_permissions(&temporary)?;
    fs::rename(temporary, path)?;
    Ok(())
}

#[cfg(unix)]
fn set_private_permissions(path: &std::path::Path) -> io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o600))
}

#[cfg(not(unix))]
fn set_private_permissions(_: &std::path::Path) -> io::Result<()> {
    Ok(())
}

pub struct SingleInstance {
    _file: fs::File,
}

impl SingleInstance {
    pub fn acquire() -> io::Result<Option<Self>> {
        use fs2::FileExt;
        let path = config_path()?.with_file_name("app.lock");
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let file = fs::OpenOptions::new()
            .create(true)
            .read(true)
            .write(true)
            .open(path)?;
        match file.try_lock_exclusive() {
            Ok(()) => Ok(Some(Self { _file: file })),
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => Ok(None),
            Err(error) => Err(error),
        }
    }
}
