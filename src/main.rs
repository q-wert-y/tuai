//! tuai 入口：初始化数据目录、日志、配置、数据库，启动事件循环。

mod app;
mod commands;
mod config;
mod llm;
mod model;
mod store;
mod tui;
mod util;

fn main() -> anyhow::Result<()> {
    // 数据目录：可执行文件所在目录下的 .tuai/
    let data_dir = util::data_dir()?;
    util::init_logging(&data_dir);
    let config = config::Config::load_or_init(&data_dir)?;
    let store = store::Store::open(&data_dir)?;

    let runtime = tokio::runtime::Runtime::new()?;
    runtime.block_on(app::run(config, store, data_dir))
}
