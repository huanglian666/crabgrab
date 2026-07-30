fn main() {
    if let Err(error) = crabgrab::cli::run_default() {
        eprintln!("error: {error}");
        std::process::exit(1);
    }
}
