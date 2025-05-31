use std::sync::OnceLock;

use tracing::info;

pub mod cfg;

pub fn main() {
    let config = cfg::init();
    cfg::init_log(&config);
    info!("Starting with configuration: {config:?}");
    CFG.set(config)
        .expect("should be settable since we hadn't yet set the global config");

    tokio::runtime::Builder::new_multi_thread()
        .thread_name("worker")
        .worker_threads(cfg().file.worker_threads as _)
        .enable_all()
        .build()
        .expect("failed to create Tokio runtime")
        .block_on(async_main());
}

pub static CFG: OnceLock<cfg::Cfg> = OnceLock::new();

fn cfg() -> &'static cfg::Cfg {
    CFG.get().expect("should be initialized by this point")
}

pub async fn async_main() {}
