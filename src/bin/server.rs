#[tokio::main]
async fn main() {
    match rust_kv_store::server::ServerConfig::parse(std::env::args()) {
        Ok(None) => {
            print!("{}", rust_kv_store::server::help_text());
        }
        Ok(Some(config)) => {
            let result = if config.sync {
                // 同步运行服务器
                rust_kv_store::server::_run(config)
            } else {
                // 异步运行服务器
                rust_kv_store::server::run(config).await
            };
            if let Err(error) = result {
                eprintln!("server error: {error}");
                std::process::exit(1);
            }
        }
        Err(error) => {
            eprintln!("argument error: {error}");
            eprintln!("{}", rust_kv_store::server::help_text());
            std::process::exit(2);
        }
    }
}
