//! Disposable native qualification wiring, not an owner activation mechanism.
#[cfg(target_os = "linux")]
mod linux {
    use aikit_sdk::integration::*;
    use serde_json::json;
    use std::{
        error::Error,
        fs,
        io::{Read, Write},
        os::unix::fs::OpenOptionsExt,
        path::PathBuf,
    };
    const HASH: &str = "356733f185fa988e6bf3aa9a555269fdd167982a2e2a80d3c8a78037bb92ad58";
    struct Allow;
    impl HookHandler for Allow {
        fn decide<'a>(&'a self, _: &'a HookRequest) -> DecisionFuture<'a> {
            Box::pin(async { Ok(Decision::Allow) })
        }
    }
    pub async fn run() -> Result<(), Box<dyn Error>> {
        let args: Vec<_> = std::env::args().skip(1).collect();
        if args.len() != 3 {
            return Err("usage: native_caller_binding setup|callback|check-original|check-current|detach-current|dump WORKSPACE STATE".into());
        }
        let workspace = fs::canonicalize(&args[1])?;
        if !workspace.join(".codelaya-qualification-only").is_file() {
            return Err("disposable marked workspace required".into());
        }
        let state = PathBuf::from(&args[2]);
        let sdk = if args[0] == "setup" {
            IntegrationService::open(&state)?
        } else {
            IntegrationService::open_existing(&state)?
        };
        if args[0] == "setup" {
            let exe = std::env::current_exe()?;
            let handler = HookCommand {
                executable: exe.clone(),
                arguments: vec![
                    "callback".into(),
                    workspace.to_string_lossy().into_owned(),
                    state.to_string_lossy().into_owned(),
                ],
            };
            let plan = sdk.plan_install(InstallSpec {
                application_id: "caller-binding-fixture".into(),
                agent_key: "claude".into(),
                workspace: workspace.clone(),
                handler: handler.clone(),
                events: vec![HookEvent::SessionStarted, HookEvent::SessionEnded],
                timeout_seconds: 10,
            })?;
            sdk.apply_install(&plan.id)?;
            let tool=sdk.plan_agent_tool(&plan.installation_id,AgentToolSpec {
                application_id:"caller-binding-fixture".into(),installation_id:plan.installation_id.clone(),tool_name:"caller_probe".into(),
                description:"Disposable caller qualification fixture".into(),input_schema:json!({"type":"object","properties":{},"additionalProperties":false}),
                instructions:"Disposable diagnostic only; no model prompt is submitted.".into(),handler,timeout_ms:10000,native_completion_bridge:false,
            })?;
            sdk.apply_install(&tool.id)?;
            println!(
                "{}",
                json!({"installed":true,"production_activated":false,"installation_id":plan.installation_id})
            );
        } else if args[0] == "callback" {
            let mut bytes = Vec::new();
            std::io::stdin().take(131073).read_to_end(&mut bytes)?;
            if bytes.len() > 131072 {
                return Err("fixture callback exceeds limit".into());
            }
            let input: serde_json::Value = serde_json::from_slice(&bytes)?;
            let installation =
                sdk.find_installation("caller-binding-fixture", "claude", &workspace)?;
            let response = sdk.handle_hook(&installation.id, &bytes, &Allow).await;
            if response.exit_code != 0 {
                return Err("native journal callback failed".into());
            }
            if input["hook_event_name"] == "SessionStart" {
                if let Ok(reference) = sdk.observed_session(
                    &installation.id,
                    input["session_id"]
                        .as_str()
                        .ok_or("native session absent")?,
                ) {
                    let binding = sdk.bind_existing(&reference)?;
                    let address = std::env::var("CLAUDE_CODE_MESSAGING_SOCKET")?;
                    let record = binding.bind_linux_native_caller(&address, HASH)?;
                    let cursor = sdk.events(&installation.id, 0, 500)?.next_cursor;
                    let ordinary_refused = binding.admit_recorded(cursor).is_err();
                    drop(binding.admit_recorded_with_linux_native_caller(cursor, &address, HASH)?);
                    match fs::OpenOptions::new()
                        .write(true)
                        .create_new(true)
                        .mode(0o600)
                        .open("/home/node/evidence/original-binding.json")
                    {
                        Ok(file) => serde_json::to_writer(file, &record)?,
                        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
                        Err(error) => return Err(error.into()),
                    }
                    fs::write(
                        "/home/node/evidence/current-binding.json",
                        serde_json::to_vec(&record)?,
                    )?;
                    let mut log = fs::OpenOptions::new()
                        .create(true)
                        .append(true)
                        .open("/home/node/evidence/bindings.jsonl")?;
                    writeln!(
                        log,
                        "{}",
                        json!({"record":record,"ordinary_refused":ordinary_refused,"inspected_admitted":true})
                    )?;
                    let cache = "/home/node/.aikit-binding-socket";
                    if !std::path::Path::new(cache).exists() {
                        fs::OpenOptions::new()
                            .write(true)
                            .create_new(true)
                            .mode(0o600)
                            .open(cache)?
                            .write_all(address.as_bytes())?;
                    }
                }
            }
            print!("{}", response.stdout);
        } else {
            let original = args[0] == "check-original";
            let name = if original { "original" } else { "current" };
            let value: serde_json::Value = serde_json::from_slice(&fs::read(format!(
                "/home/node/evidence/{name}-binding.json"
            ))?)?;
            let binding = sdk.binding(
                value["binding_id"]
                    .as_str()
                    .ok_or("fixture binding absent")?,
            )?;
            match args[0].as_str() {
                "check-original" | "check-current" => {
                    let address = std::env::var("CLAUDE_CODE_MESSAGING_SOCKET")
                        .or_else(|_| fs::read_to_string("/home/node/.aikit-binding-socket"))?;
                    let cursor = binding.events(0, 500)?.next_cursor;
                    drop(binding.admit_recorded_with_linux_native_caller(cursor, &address, HASH)?);
                    println!("{}", json!({"admitted":true,"binding_id":binding.id()}));
                }
                "detach-current" => {
                    binding.detach()?;
                    println!("{}", json!({"detached":true,"binding_id":binding.id()}));
                }
                "dump" => {
                    println!(
                        "{}",
                        json!({"status":binding.status()?,"record":binding.recorded_linux_native_caller()?})
                    );
                }
                _ => return Err("unknown fixture action".into()),
            }
        }
        Ok(())
    }
}
#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    #[cfg(target_os = "linux")]
    {
        let result = linux::run().await;
        if let Err(error) = &result {
            let _ = std::fs::write(
                "/home/node/evidence/fixture-error.txt",
                format!("{error:?}"),
            );
        }
        result
    }
    #[cfg(not(target_os = "linux"))]
    Err("native caller binding qualification is Linux-only".into())
}
