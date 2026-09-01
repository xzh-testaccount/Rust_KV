fn main() {
    if let Err(error) = rust_kv_store::server::run(Default::default()) {
        eprintln!("kv-server: {error}");
        std::process::exit(1);
    }
}
