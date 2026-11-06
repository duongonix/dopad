fn main() {
    let code = dopad_cli::run::run(std::env::args_os().skip(1));
    std::process::exit(code);
}
