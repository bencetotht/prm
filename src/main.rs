fn main() {
    if let Err(err) = prm::cli::run() {
        eprintln!("error: {err:#}");
        std::process::exit(1);
    }
}
