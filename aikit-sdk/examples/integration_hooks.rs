//! Library-only integration example. The example never launches an agent.
//! Run with --features integration. Usage is described in the SDK README.
use aikit_sdk::integration::{
    Decision, DecisionFuture, HookEvent, HookHandler, HookRequest, InstallSpec, IntegrationService,
    SessionMode,
};
use serde_json::{json, Value};
use std::{
    io::{Read, Write},
    path::Path,
};

struct ExampleGate<'a> {
    service: &'a IntegrationService,
    blocks: usize,
    deny_tools: bool,
}
impl HookHandler for ExampleGate<'_> {
    fn decide<'a>(&'a self, request: &'a HookRequest) -> DecisionFuture<'a> {
        Box::pin(async move {
            if self.deny_tools && request.event == HookEvent::BeforeTool {
                return Ok(Decision::Block {
                    reason: "Example qualification gate denies tool execution. Do not retry or use an alternative tool; report BLOCKED.".into(),
                });
            }
            if request.event != HookEvent::CompletionProposed {
                return Ok(Decision::Allow);
            }
            let mut cursor = 0;
            let mut proposals = 0;
            let reference = self
                .service
                .observed_session(&request.installation_id, &request.session_id)
                .map_err(|e| aikit_sdk::integration::HandlerError(e.to_string()))?;
            let binding = self
                .service
                .bind_existing(&reference)
                .map_err(|e| aikit_sdk::integration::HandlerError(e.to_string()))?;
            loop {
                let page = binding
                    .events(cursor, 100)
                    .map_err(|e| aikit_sdk::integration::HandlerError(e.to_string()))?;
                for record in &page.records {
                    if record.request.event == HookEvent::CompletionProposed
                        && record.decision.is_none()
                    {
                        proposals += 1;
                    }
                }
                if page.next_cursor == cursor {
                    break;
                }
                cursor = page.next_cursor;
            }
            if proposals <= self.blocks
                || request
                    .final_answer
                    .as_deref()
                    .unwrap_or("")
                    .trim()
                    .is_empty()
            {
                Ok(Decision::Block { reason: "Example qualification gate is pending. Continue and respond OK again without using tools.".into() })
            } else {
                Ok(Decision::Allow)
            }
        })
    }
}

#[tokio::main(flavor = "current_thread")]
async fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let is_hook = matches!(
        args.first().map(String::as_str),
        Some("hook" | "hook-cursor" | "hook-codex" | "hook-pi")
    );
    let result = run(&args).await;
    match result {
        Ok(code) => std::process::exit(code),
        Err(error) => {
            if is_hook {
                eprintln!(
                    "Integration handler unavailable; restore its local state before retrying."
                );
            } else {
                eprintln!("{error}");
            }
            std::process::exit(if is_hook { 2 } else { 1 });
        }
    }
}

async fn run(args: &[String]) -> anyhow::Result<i32> {
    if args.len() < 2 {
        anyhow::bail!("usage: integration_hooks <capabilities|plan|apply|remove-plan|status|events|hook|bind|binding-status|detach> STATE [ID | WORKSPACE BLOCKS | INSTALLATION_ID SESSION_ID | AGENT_KEY MODE]");
    }
    let service = IntegrationService::open(&args[1])?;
    if matches!(
        args[0].as_str(),
        "hook" | "hook-cursor" | "hook-codex" | "hook-pi"
    ) {
        if !(args.len() == 4 || (args.len() == 5 && args[4] == "deny-tools")) {
            anyhow::bail!("hook requires STATE WORKSPACE BLOCKS [deny-tools]");
        }
        let agent_key = if args[0] == "hook-cursor" {
            "cursor"
        } else if args[0] == "hook-codex" {
            "codex"
        } else if args[0] == "hook-pi" {
            "pi"
        } else {
            "claude"
        };
        let installation =
            service.find_installation("sdk-example", agent_key, Path::new(&args[2]))?;
        let gate = ExampleGate {
            service: &service,
            blocks: args[3].parse()?,
            deny_tools: args.len() == 5,
        };
        let mut input = Vec::new();
        std::io::stdin()
            .take(1024 * 1024 + 1)
            .read_to_end(&mut input)?;
        let response = service.handle_hook(&installation.id, &input, &gate).await;
        std::io::stdout().write_all(response.stdout.as_bytes())?;
        std::io::stderr().write_all(response.stderr.as_bytes())?;
        return Ok(response.exit_code);
    }
    let value: Value = match args[0].as_str() {
        "capabilities" => {
            let key = args
                .get(2)
                .ok_or_else(|| anyhow::anyhow!("missing agent key"))?;
            let mode = match args.get(3).map(String::as_str) {
                Some("print") => SessionMode::Print,
                Some("interactive") => SessionMode::Interactive,
                Some("unknown") | None => SessionMode::Unknown,
                _ => anyhow::bail!("mode must be print, interactive or unknown"),
            };
            serde_json::to_value(service.capabilities(key, mode)?)?
        }
        "plan" => {
            let mut input = String::new();
            std::io::stdin()
                .take(1024 * 1024 + 1)
                .read_to_string(&mut input)?;
            if input.len() > 1024 * 1024 {
                anyhow::bail!("spec exceeds 1 MiB");
            }
            let spec: InstallSpec = serde_json::from_str(&input)?;
            serde_json::to_value(service.plan_install(spec)?)?
        }
        "apply" => serde_json::to_value(
            service.apply_install(
                args.get(2)
                    .ok_or_else(|| anyhow::anyhow!("missing plan ID"))?,
            )?,
        )?,
        "remove-plan" => serde_json::to_value(
            service.plan_remove(
                args.get(2)
                    .ok_or_else(|| anyhow::anyhow!("missing installation ID"))?,
            )?,
        )?,
        "status" => serde_json::to_value(
            service.installation_status(
                args.get(2)
                    .ok_or_else(|| anyhow::anyhow!("missing installation ID"))?,
            )?,
        )?,
        "events" => {
            let id = args
                .get(2)
                .ok_or_else(|| anyhow::anyhow!("missing installation ID"))?;
            let mut records = Vec::new();
            let mut cursor = 0;
            loop {
                let page = service.events(id, cursor, 100)?;
                records.extend(page.records);
                if page.next_cursor == cursor {
                    break;
                }
                cursor = page.next_cursor;
            }
            json!({"records": records, "next_cursor":cursor})
        }
        "bind" => {
            let reference = service.observed_session(
                args.get(2)
                    .ok_or_else(|| anyhow::anyhow!("missing installation ID"))?,
                args.get(3)
                    .ok_or_else(|| anyhow::anyhow!("missing native session ID"))?,
            )?;
            let binding = service.bind_existing(&reference)?;
            json!({"id":binding.id(), "reference":binding.reference(), "status":binding.status()?})
        }
        "binding-status" => serde_json::to_value(
            service
                .binding(
                    args.get(2)
                        .ok_or_else(|| anyhow::anyhow!("missing binding ID"))?,
                )?
                .status()?,
        )?,
        "detach" => {
            let binding = service.binding(
                args.get(2)
                    .ok_or_else(|| anyhow::anyhow!("missing binding ID"))?,
            )?;
            binding.detach()?;
            serde_json::to_value(binding.status()?)?
        }
        _ => anyhow::bail!("unknown operation"),
    };
    println!("{}", serde_json::to_string_pretty(&value)?);
    Ok(0)
}
