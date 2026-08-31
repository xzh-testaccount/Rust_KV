fn main() {
    match rust_kv_store::client::ClientConfig::parse(std::env::args()) {
        Ok(None) => {
            print!("{}", rust_kv_store::client::help_text());
        }
        Ok(Some(config)) => {
            if let Err(error) = rust_kv_store::client::run(config) {
                eprintln!("client error: {error}");
                std::process::exit(1);
            }
        }
        Err(error) => {
            eprintln!("argument error: {error}");
            eprintln!("{}", rust_kv_store::client::help_text());
            std::process::exit(2);
        }
    }
}
