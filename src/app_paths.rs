//! Installation resources are immutable; all saves belong to the user's data root.
use std::path::{Path, PathBuf};

mod migration;
mod validation;

#[derive(Debug)]
pub struct ResourceLayout {
    pub assets: PathBuf,
    pub defaults: PathBuf,
    pub legacy_configs: Option<PathBuf>,
    pub version: Option<PathBuf>,
}

impl ResourceLayout {
    pub fn discover() -> Result<Self, String> {
        let exe = std::env::current_exe().map_err(|e| format!("Cannot locate executable: {e}"))?;
        let mut layout = Self::resolve(
            &exe,
            Some(Path::new(env!("CARGO_MANIFEST_DIR"))),
            option_env!("MOTOR_TRAIL_DISTRIBUTION") == Some("1"),
        )?;
        if layout.version.is_none() {
            if let Some(root) = std::env::var_os("BEVY_ASSET_ROOT") {
                let root = PathBuf::from(root);
                if !root.is_absolute() {
                    return Err("BEVY_ASSET_ROOT must be absolute".into());
                }
                layout.assets = root.join("assets");
            }
        }
        Ok(layout)
    }

    pub fn resolve(
        exe: &Path,
        development: Option<&Path>,
        distribution: bool,
    ) -> Result<Self, String> {
        if !exe.is_absolute() {
            return Err("Executable path must be absolute".into());
        }
        let parent = exe.parent().ok_or("Executable has no parent directory")?;
        if parent.file_name().is_some_and(|name| name == "MacOS")
            && parent
                .parent()
                .is_some_and(|contents| contents.file_name().is_some_and(|name| name == "Contents"))
        {
            return Self::packaged(&parent.parent().unwrap().join("Resources"));
        }
        if distribution
            || parent.join("version.json").exists()
            || parent.join("assets").is_dir()
            || parent.join("defaults").is_dir()
        {
            return Self::packaged(parent);
        }
        if let Some(root) = development.filter(|root| {
            root.is_absolute() && root.join("assets").is_dir() && root.join("configs").is_dir()
        }) {
            return Ok(Self {
                assets: root.join("assets"),
                defaults: root.join("configs"),
                legacy_configs: Some(root.join("configs")),
                version: None,
            });
        }
        Err(format!(
            "Cannot find Motor-Trail resources beside {}",
            exe.display()
        ))
    }

    fn packaged(root: &Path) -> Result<Self, String> {
        let layout = Self {
            assets: root.join("assets"),
            defaults: root.join("defaults"),
            legacy_configs: root.join("configs").is_dir().then(|| root.join("configs")),
            version: Some(root.join("version.json")),
        };
        if !layout.assets.is_dir()
            || !layout.defaults.join("vehicles").is_dir()
            || !layout.defaults.join("terrains").is_dir()
            || !root.join("version.json").is_file()
        {
            return Err(format!(
                "Incomplete Motor-Trail installation at {}: assets, defaults and version.json are required",
                root.display()
            ));
        }
        Ok(layout)
    }

    pub fn initialize_user_data(&self, root: &Path) -> Result<(), String> {
        migration::initialize(self, root)
    }

    pub fn check_installation(&self) -> Result<String, String> {
        validation::check(self)
    }
}

#[derive(Clone, Copy)]
#[allow(dead_code)] // All variants are exercised on every host by path tests.
pub enum Platform {
    Windows,
    MacOs,
    Linux,
}

pub fn user_root(
    platform: Platform,
    appdata: Option<&Path>,
    home: Option<&Path>,
    xdg: Option<&Path>,
) -> Result<PathBuf, String> {
    let (base, suffix) = match platform {
        Platform::Windows => (appdata, "MotorTrail"),
        Platform::MacOs => (home, "Library/Application Support/MotorTrail"),
        Platform::Linux if xdg.is_some() => (xdg, "MotorTrail"),
        Platform::Linux => (home, ".local/share/MotorTrail"),
    };
    let base = base
        .filter(|base| base.is_absolute())
        .ok_or("An absolute OS user-data directory is required")?;
    Ok(base.join(suffix))
}

pub fn native_user_root() -> Result<PathBuf, String> {
    let appdata = std::env::var_os("APPDATA").map(PathBuf::from);
    let home = std::env::var_os("HOME").map(PathBuf::from);
    let xdg = std::env::var_os("XDG_DATA_HOME")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from);
    let platform = if cfg!(target_os = "windows") {
        Platform::Windows
    } else if cfg!(target_os = "macos") {
        Platform::MacOs
    } else {
        Platform::Linux
    };
    user_root(
        platform,
        appdata.as_deref(),
        home.as_deref(),
        xdg.as_deref(),
    )
}

pub fn command_line_data_root() -> Result<PathBuf, String> {
    let mut args = std::env::args_os().skip(1);
    let mut selected = None;
    while let Some(arg) = args.next() {
        if arg == "--user-data-dir" {
            if selected.is_some() {
                return Err("--user-data-dir was supplied twice".into());
            }
            let value = args
                .next()
                .filter(|value| !value.is_empty() && !value.to_string_lossy().starts_with("--"))
                .ok_or("--user-data-dir requires a directory")?;
            let path = PathBuf::from(value);
            selected = Some(if path.is_absolute() {
                path
            } else {
                std::env::current_dir()
                    .map_err(|e| format!("Cannot resolve --user-data-dir: {e}"))?
                    .join(path)
            });
        }
    }
    selected.map_or_else(native_user_root, Ok)
}

#[cfg(test)]
mod tests;
