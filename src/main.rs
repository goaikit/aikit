mod cli;
mod core;
mod error;
mod fs;
mod git;
mod github;
mod models;
#[cfg(feature = "tools")]
mod tools;
mod tui;

#[tokio::main]
async fn main() {
    let _ = dotenv::dotenv();

    // Scan for --debug before handing args to cli-framework (no global flag support)
    let mut raw: Vec<String> = std::env::args().collect();
    let debug = raw.iter().any(|a| a == "--debug");
    raw.retain(|a| a != "--debug");

    if debug {
        std::env::set_var("AIKIT_DEBUG", "1");
        eprintln!("[DEBUG] Debug mode enabled");
        init_tracing(true);
    } else {
        init_tracing(false);
    }

    let mut app = match cli::build_app() {
        Ok(a) => a,
        Err(e) => {
            eprintln!("Error: {}", e);
            std::process::exit(1);
        }
    };

    if let Err(e) = app.run_with_args(raw).await {
        eprintln!("Error: {}", e);
        std::process::exit(1);
    }
}

fn init_tracing(debug: bool) {
    use tracing_subscriber::EnvFilter;
    let rust_log = std::env::var_os("RUST_LOG").is_some();
    if !rust_log && !debug {
        return;
    }
    let filter = if rust_log {
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("warn"))
    } else {
        EnvFilter::new("aikit_sdk=debug,warn")
    };
    let _ = tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_target(true)
        .with_writer(std::io::stderr)
        .try_init();
}
