fn main() {
    match rust_kv_store::server::ServerConfig::parse(std::env::args()) {
        Ok(None) => print!("{}", rust_kv_store::server::help_text()),
        Ok(Some(config)) => {
            if let Err(error) = rust_kv_store::server::run(config) {
                eprintln!("kv-server: {error}");
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
