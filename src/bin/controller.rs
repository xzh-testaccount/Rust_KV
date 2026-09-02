use rust_kv_store::controller::{ControllerConfig, help_text};

fn main() {
    let config = match ControllerConfig::parse(std::env::args()) {
        Ok(Some(config)) => config,
        Ok(None) => {
            print!("{}", help_text());
            return;
        }
        Err(error) => {
            eprintln!("kv-controller: {error}");
            std::process::exit(2);
        }
    };

    if let Err(error) = rust_kv_store::controller::run(config) {
        eprintln!("kv-controller: {error}");
        std::process::exit(1);
    }
}
