use std::env;
use std::path::PathBuf;

pub fn blindeye_data_dir() -> PathBuf {
    if let Some(dir) = windows_data_dir() {
        return dir.join("BlindEye");
    }

    if let Some(dir) = unix_data_dir() {
        return dir.join("blindeye");
    }

    PathBuf::from(".")
}

fn windows_data_dir() -> Option<PathBuf> {
    #[cfg(target_os = "windows")]
    {
        env::var_os("LOCALAPPDATA")
            .or_else(|| env::var_os("APPDATA"))
            .map(PathBuf::from)
            .or_else(|| {
                env::var_os("USERPROFILE").map(|profile| {
                    PathBuf::from(profile).join("AppData").join("Local")
                })
            })
    }

    #[cfg(not(target_os = "windows"))]
    {
        None
    }
}

fn unix_data_dir() -> Option<PathBuf> {
    env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .or_else(|| {
            env::var_os("HOME").map(|home| {
                PathBuf::from(home).join(".local").join("share")
            })
        })
}
