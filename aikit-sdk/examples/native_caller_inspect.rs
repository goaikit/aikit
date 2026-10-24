//! Thin diagnostic for the reusable Linux kernel peer/ancestor inspector.
fn main() -> Result<(), Box<dyn std::error::Error>> {
    #[cfg(target_os = "linux")]
    {
        let args: Vec<_> = std::env::args().skip(1).collect();
        if args.len() != 2 {
            return Err(
                "usage: native_caller_inspect SOCKET EXPECTED_NATIVE_EXECUTABLE_SHA256".into(),
            );
        }
        let facts = aikit_sdk::integration::inspect_linux_native_caller(&args[0], &args[1])?;
        println!("{}", serde_json::to_string(&facts)?);
        Ok(())
    }
    #[cfg(not(target_os = "linux"))]
    Err("native caller inspection is unqualified on this platform".into())
}
