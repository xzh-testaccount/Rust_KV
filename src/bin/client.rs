fn main() {
    if let Err(error) = rust_kv_store::client::run(Default::default()) {
        eprintln!("kv-client: {error}");
        std::process::exit(1);
    }
}
