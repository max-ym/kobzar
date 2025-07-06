use std::{
    net::SocketAddr,
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};
use tracing::info;

#[derive(Debug)]
pub struct Cfg {
    file: ConfigFile,
    env: ConfigEnv,
    worker_threads: usize,
}

pub fn init() -> Cfg {
    let env = ConfigEnv::load();
    let file = ConfigFile::load(&env);

    let worker_threads = if file.worker_threads == 0 {
        num_cpus::get()
    } else {
        file.worker_threads as usize
    };

    Cfg {
        file,
        env,
        worker_threads,
    }
}

impl Cfg {
    /// Get the address and port to bind the server to.
    pub fn bind(&self) -> SocketAddr {
        self.env.bind
    }

    /// Get the path to store system database files.
    pub fn syspath(&self) -> &PathBuf {
        &self.env.syspath
    }

    /// Get the root user for the system.
    pub fn root_usr(&self) -> &str {
        &self.env.root_usr
    }

    /// Get the root password for the system.
    pub fn root_pwd(&self) -> &str {
        &self.env.root_pwd
    }

    /// Get the root key for the system.
    pub fn root_key(&self) -> &str {
        &self.env.root_key
    }

    pub fn worker_threads(&self) -> usize {
        self.worker_threads
    }

    /// Find the device configuration for the given path.
    /// We move from most specific to least specific path,
    /// so that if there are multiple devices with similar path,
    /// one being a child of another,
    /// the most specific one is returned.
    pub fn device_at(&self, path: impl AsRef<Path>) -> &DeviceConfig {
        let mut best: Option<(&DeviceConfig, usize)> = None;

        for device in &self.file.io_device {
            if path.as_ref().starts_with(&device.path) {
                let cnt = device.path.iter().count();
                if let Some((_, best_cnt)) = best {
                    if best_cnt < cnt {
                        best = Some((device, cnt));
                    }
                } else {
                    best = Some((device, cnt));
                }
            }
        }

        if let Some((device, _)) = best {
            device
        } else {
            // If no device is found, return the default device configuration.
            // Normally this should not happen, as the default should be set
            // on configuration load, but we provide it as fallback just in case.
            use std::sync::OnceLock;
            static DEFAULT_DEVICE: OnceLock<DeviceConfig> = OnceLock::new();
            DEFAULT_DEVICE.get_or_init(DeviceConfig::default)
        }
    }
}

/// Configuration for the server, which can be loaded from a configuration file.
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct ConfigFile {
    /// Number of worker threads to execute the server logic.
    /// If set to 0, the number of worker threads is equal to the number of CPU cores.
    pub worker_threads: u32,

    /// Maximum number of connections to the server.
    /// If not set, the default value is 1024.
    pub max_connections: u32,

    /// Per-IO device configuration.
    #[serde(default)]
    pub io_device: Vec<DeviceConfig>,

    /// Soft maximum size of the Write-Ahead Log (WAL) fragment in bytes.
    pub wal_frag_soft_max_bytes: u64,

    /// Minimum size of the Write-Ahead Log (WAL) fragment in bytes.
    /// The log won't be split into smaller fragments than this size.
    pub wal_frag_min_bytes: u64,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DeviceConfig {
    /// Path to the device's mounted path. All IO operations on this path will have the following
    /// configuration as per this section.
    ///
    /// For example, if the device is mounted at `/mnt/device`, then the path should be `/mnt/device`.
    pub path: PathBuf,

    /// Maximum concurrent IO operations on the device.
    /// If not set, the default value is 1.
    pub concurrency: Option<u32>,

    /// When large updates are made, try to combine them into larger IO operations
    /// to reduce the number of IO operations. This parameters sets the size of the
    /// combined IO in bytes.
    pub io_combine_bytes: u64,
}

impl Default for DeviceConfig {
    fn default() -> Self {
        Self {
            path: PathBuf::from("/"),
            concurrency: Some(1),
            io_combine_bytes: 8192,
        }
    }
}

/// Configuration for the server, which can be loaded from environment variables.
#[derive(Debug)]
pub struct ConfigEnv {
    /// Address and port to bind the server to.
    /// For example, "127.0.0.1:5435"
    pub bind: SocketAddr,

    /// Path to store system database files.
    pub syspath: PathBuf,

    /// Root user for the system.
    pub root_usr: String,

    /// Root password for the system. Can either be a plain text password or
    /// bcrypt hash. Can be empty, then password authentication is disabled.
    pub root_pwd: String,

    /// Key file for the root user. Can be empty, then key authentication is disabled.
    /// If both [Self::root_pwd] and [Self::root_key] are empty, root user cannot be
    /// authenticated.
    /// If [Self::root_pwd] is also set, both password and key authentication
    /// are used at the same time to authenticate the root user.
    pub root_key: String,

    /// Log level for the server. Default is [INFO](tracing::Level::INFO).
    pub log_level: tracing::Level,
}

impl ConfigEnv {
    /// Create a new configuration from environment variables.
    #[tracing::instrument(skip_all)]
    pub fn load() -> Self {
        use std::env::var;
        if dotenv::dotenv().is_err() {
            info!("Failed to load .env file");
        }

        Self {
            bind: var("BIND")
                .map(|v| v.parse())
                .unwrap_or_else(|_| Ok(SocketAddr::from(([127, 0, 0, 1], 5435))))
                .expect("Failed to parse BIND environment variable"),
            syspath: var("SYSPATH")
                .map(Into::into)
                .expect("Failed to parse SYSPATH environment variable"),
            root_usr: var("ROOT_USR").unwrap_or_else(|_| "root".into()),
            root_pwd: var("ROOT_PWD").unwrap_or_default(),
            root_key: var("ROOT_KEY").unwrap_or_default(),
            log_level: var("LOG_LEVEL")
                .map(|v| v.parse())
                .unwrap_or_else(|_| Ok(tracing::Level::INFO))
                .expect("Failed to parse LOG_LEVEL environment variable"),
        }
    }
}

impl ConfigFile {
    /// Create a new configuration from a file.
    #[tracing::instrument(skip_all)]
    pub fn load(env: &ConfigEnv) -> Self {
        // Check that the file exists, else create a new default configuration file.
        let config_path = env.syspath.join("config.toml");
        if !config_path.exists() {
            let cfg = Self::default();
            info!(
                "Configuration file not found, creating a new one at {:?}",
                config_path
            );
            std::fs::write(&config_path, toml::to_string(&cfg).unwrap())
                .expect("Failed to create configuration file");
            cfg
        } else {
            let content =
                std::fs::read_to_string(&config_path).expect("Failed to read configuration file");
            toml::from_str(&content).expect("Failed to parse configuration file")
        }
    }
}

impl Default for ConfigFile {
    fn default() -> Self {
        Self {
            worker_threads: 0,
            max_connections: 1024,
            io_device: vec![DeviceConfig::default()],
            wal_frag_soft_max_bytes: 1024 * 1024 * 10, // 10 MB
            wal_frag_min_bytes: 1024 * 1024,           // 1 MB
        }
    }
}

pub fn init_log(cfg: &Cfg) {
    let log_level = if cfg!(test) {
        log::LevelFilter::Trace
    } else {
        log_level_filter(cfg.env.log_level)
    };

    fern::Dispatch::new()
        .level(log::LevelFilter::Warn)
        .level_for(env!("CARGO_PKG_NAME"), log_level)
        .chain(std::io::stdout())
        .apply()
        .expect("failed to set global logger");
}

fn log_level_filter(lvl: tracing::Level) -> log::LevelFilter {
    match lvl {
        tracing::Level::TRACE => log::LevelFilter::Trace,
        tracing::Level::DEBUG => log::LevelFilter::Debug,
        tracing::Level::INFO => log::LevelFilter::Info,
        tracing::Level::WARN => log::LevelFilter::Warn,
        tracing::Level::ERROR => log::LevelFilter::Error,
    }
}
