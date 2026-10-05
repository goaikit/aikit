fn main() {
    println!(
        "{}",
        serde_json::to_string_pretty(&aikit_sdk::runner::session::schemas()).unwrap()
    );
}
