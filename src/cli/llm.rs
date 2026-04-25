use std::io::Write;
use std::time::Instant;

use anyhow::Result;
use clap::Parser;
use tokio_stream::StreamExt;

use crate::core::llm_http::{
    build_chat_request, build_client, build_json_envelope, extract_content_from_chunk,
    extract_usage_from_chunk, format_usage_stderr, parse_sse_line, resolve_api_key, resolve_prompt,
    send_chat, ChatRequest, JsonUsage, LlmError, SseChunk, StreamEvent,
};

#[derive(Parser, Debug)]
#[command(about = "Invoke an LLM via OpenAI-compatible API", group = clap::ArgGroup::new("prompt_input").required(true))]
pub struct LlmArgs {
    #[arg(long, short = 'm', value_name = "MODEL")]
    pub model: String,

    #[arg(
        long,
        short = 'u',
        value_name = "URL",
        default_value = "https://api.openai.com/v1"
    )]
    pub url: String,

    #[arg(long, short = 'p', group = "prompt_input", value_name = "TEXT")]
    pub prompt: Option<String>,

    #[arg(long, group = "prompt_input", value_name = "FILE")]
    pub prompt_file: Option<String>,

    #[arg(long, value_name = "SYSTEM")]
    pub system: Option<String>,

    #[arg(long)]
    pub stream: bool,

    #[arg(long, short = 'o', value_name = "FILE")]
    pub out: Option<String>,

    #[arg(long, value_name = "FORMAT", default_value = "text")]
    pub format: String,

    #[arg(long, value_name = "TOKENS")]
    pub max_tokens: Option<u32>,

    #[arg(long, value_name = "TEMP")]
    pub temperature: Option<f64>,

    #[arg(long, value_name = "TOP_P")]
    pub top_p: Option<f64>,

    #[arg(long, value_name = "ENV_VAR")]
    pub api_key_env: Option<String>,

    #[arg(long, value_name = "SECS", default_value_t = 120)]
    pub timeout: u64,

    #[arg(long, value_name = "SECS", default_value_t = 10)]
    pub connect_timeout: u64,

    #[arg(long, value_name = "MODE")]
    pub usage: Option<String>,

    #[arg(long)]
    pub quiet: bool,
}

pub async fn execute(args: LlmArgs) -> Result<()> {
    let is_json = args.format == "json";
    let usage_mode = args.usage.as_deref();
    let quiet = args.quiet;

    let api_key = resolve_api_key(args.api_key_env.as_deref()).map_err(|e| {
        if is_json {
            let event = StreamEvent::error(1, "E_LLM_AUTH_MISSING".to_string(), e.to_string());
            let _ = writeln!(
                std::io::stdout(),
                "{}",
                serde_json::to_string(&event).unwrap()
            );
        }
        anyhow::anyhow!("{}", e)
    })?;

    let prompt_text =
        resolve_prompt(args.prompt.as_deref(), args.prompt_file.as_deref()).map_err(|e| {
            let code = match &e {
                LlmError::PromptMissing => "E_LLM_PROMPT_MISSING",
                LlmError::PromptConflict => "E_LLM_PROMPT_CONFLICT",
                LlmError::PromptFileRead { .. } => "E_LLM_PROMPT_FILE_READ",
                _ => "E_LLM_UNKNOWN",
            };
            if is_json {
                let event = StreamEvent::error(1, code.to_string(), e.to_string());
                let _ = writeln!(
                    std::io::stdout(),
                    "{}",
                    serde_json::to_string(&event).unwrap()
                );
            }
            anyhow::anyhow!("{}", e)
        })?;

    let chat_req = build_chat_request(
        &args.model,
        &prompt_text,
        args.system.as_deref(),
        args.stream,
        args.max_tokens,
        args.temperature,
        args.top_p,
    );

    let client = build_client(args.timeout, args.connect_timeout).map_err(|e| {
        if is_json {
            let event = StreamEvent::error(1, "E_LLM_REQUEST_FAILED".to_string(), e.to_string());
            let _ = writeln!(
                std::io::stdout(),
                "{}",
                serde_json::to_string(&event).unwrap()
            );
        }
        anyhow::anyhow!("{}", e)
    })?;

    if args.stream {
        execute_stream(
            &client, &args, &chat_req, &api_key, is_json, quiet, usage_mode,
        )
        .await
    } else {
        execute_non_stream(
            &client, &args, &chat_req, &api_key, is_json, quiet, usage_mode,
        )
        .await
    }
}

async fn execute_non_stream(
    client: &reqwest::Client,
    args: &LlmArgs,
    chat_req: &ChatRequest,
    api_key: &str,
    is_json: bool,
    quiet: bool,
    usage_mode: Option<&str>,
) -> Result<()> {
    let start = Instant::now();
    let response = send_chat(client, &args.url, chat_req, api_key, args.timeout)
        .await
        .map_err(|e| {
            let code = match &e {
                LlmError::Timeout { .. } => "E_LLM_TIMEOUT",
                LlmError::RequestFailed { .. } => "E_LLM_REQUEST_FAILED",
                LlmError::ResponseError { .. } => "E_LLM_RESPONSE_ERROR",
                _ => "E_LLM_UNKNOWN",
            };
            if is_json {
                let event = StreamEvent::error(1, code.to_string(), e.to_string());
                let _ = writeln!(
                    std::io::stdout(),
                    "{}",
                    serde_json::to_string(&event).unwrap()
                );
            }
            anyhow::anyhow!("{}", e)
        })?;
    let latency_ms = start.elapsed().as_millis() as u64;

    if is_json {
        let envelope = build_json_envelope(&response, &args.url, latency_ms);
        let json_str = serde_json::to_string(&envelope).unwrap();
        if let Some(ref out_path) = args.out {
            write_output_file(out_path, &json_str)?;
        }
        println!("{}", json_str);
    } else {
        let text = crate::core::llm_http::render_text_response(&response);
        if let Some(ref out_path) = args.out {
            write_output_file(out_path, &text)?;
        }
        println!("{}", text);

        let effective_usage = resolve_effective_usage_mode(usage_mode, is_json);
        if effective_usage != "none" {
            if let Some(ref usage) = response.usage {
                match effective_usage {
                    "stderr" => {
                        if !quiet {
                            eprintln!("{}", format_usage_stderr(usage));
                        }
                    }
                    "json" => {
                        let json_usage = JsonUsage {
                            input_tokens: usage.prompt_tokens.unwrap_or(0),
                            output_tokens: usage.completion_tokens.unwrap_or(0),
                            total_tokens: usage.total_tokens.unwrap_or(0),
                        };
                        println!("{}", serde_json::to_string(&json_usage).unwrap());
                    }
                    _ => {}
                }
            }
        }
    }

    Ok(())
}

async fn execute_stream(
    client: &reqwest::Client,
    args: &LlmArgs,
    chat_req: &ChatRequest,
    api_key: &str,
    is_json: bool,
    quiet: bool,
    usage_mode: Option<&str>,
) -> Result<()> {
    let url = format!("{}/chat/completions", args.url.trim_end_matches('/'));

    let response = client
        .post(&url)
        .header("Authorization", format!("Bearer {}", api_key))
        .header("Content-Type", "application/json")
        .json(chat_req)
        .send()
        .await
        .map_err(|e| {
            let (code, msg) = if e.is_timeout() {
                ("E_LLM_TIMEOUT", e.to_string())
            } else {
                ("E_LLM_REQUEST_FAILED", e.to_string())
            };
            if is_json {
                let event = StreamEvent::error(1, code.to_string(), msg.clone());
                let _ = writeln!(
                    std::io::stdout(),
                    "{}",
                    serde_json::to_string(&event).unwrap()
                );
            }
            anyhow::anyhow!("{}", msg)
        })?;

    let status = response.status();
    if !status.is_success() {
        let body_text = response.text().await.unwrap_or_default();
        let err = LlmError::ResponseError {
            status: status.as_u16(),
            url: url.clone(),
            body: body_text.clone(),
        };
        if is_json {
            let event = StreamEvent::error(
                1,
                "E_LLM_RESPONSE_ERROR".to_string(),
                format!("HTTP {} from {}: {}", status.as_u16(), url, body_text),
            );
            let _ = writeln!(
                std::io::stdout(),
                "{}",
                serde_json::to_string(&event).unwrap()
            );
        }
        return Err(anyhow::anyhow!("{}", err));
    }

    let mut stream = response.bytes_stream();
    let mut seq: u64 = 1;
    let mut buffer = String::new();
    let mut full_content = String::new();
    let mut final_usage: Option<JsonUsage> = None;
    let mut line_number: u64 = 0;

    while let Some(chunk_result) = stream.next().await {
        let chunk = chunk_result.map_err(|e| {
            let err_msg = e.to_string();
            if is_json {
                let event =
                    StreamEvent::error(seq, "E_LLM_STREAM_PROTOCOL".to_string(), err_msg.clone());
                let _ = writeln!(
                    std::io::stdout(),
                    "{}",
                    serde_json::to_string(&event).unwrap()
                );
            }
            anyhow::anyhow!("E_LLM_STREAM_PROTOCOL: {}", err_msg)
        })?;

        buffer.push_str(&String::from_utf8_lossy(&chunk));

        while let Some(newline_pos) = buffer.find('\n') {
            let line = buffer[..newline_pos].to_string();
            buffer = buffer[newline_pos + 1..].to_string();
            line_number += 1;

            let parsed = parse_sse_line(&line);
            match parsed {
                Some("[DONE]") => {
                    if is_json {
                        if let Some(ref usage) = final_usage {
                            let usage_evt = StreamEvent::usage_event(seq, usage.clone());
                            println!("{}", serde_json::to_string(&usage_evt).unwrap());
                            seq += 1;
                        }
                        let done_evt = StreamEvent::done(seq);
                        println!("{}", serde_json::to_string(&done_evt).unwrap());
                    } else {
                        if let Some(ref out_path) = args.out {
                            write_output_file(out_path, &full_content)?;
                        }

                        let effective_usage = resolve_effective_usage_mode(usage_mode, is_json);
                        if effective_usage == "stderr" && !quiet {
                            if let Some(ref usage) = final_usage {
                                let u = crate::core::llm_http::Usage {
                                    prompt_tokens: Some(usage.input_tokens),
                                    completion_tokens: Some(usage.output_tokens),
                                    total_tokens: Some(usage.total_tokens),
                                };
                                eprintln!("{}", format_usage_stderr(&u));
                            }
                        }
                    }
                    return Ok(());
                }
                Some(json_str) => {
                    let sse_chunk: SseChunk = match serde_json::from_str(json_str) {
                        Ok(c) => c,
                        Err(e) => {
                            if is_json {
                                let event = StreamEvent::error(
                                    seq,
                                    "E_LLM_STREAM_PROTOCOL".to_string(),
                                    format!("invalid JSON at line {}: {}", line_number, e),
                                );
                                let _ = writeln!(
                                    std::io::stdout(),
                                    "{}",
                                    serde_json::to_string(&event).unwrap()
                                );
                            }
                            if !quiet {
                                eprintln!(
                                    "E_LLM_STREAM_PROTOCOL: invalid stream event at line {}: {}",
                                    line_number, e
                                );
                            }
                            return Err(anyhow::anyhow!(
                                "E_LLM_STREAM_PROTOCOL: invalid stream event at line {}: {}",
                                line_number,
                                e
                            ));
                        }
                    };

                    if let Some(usage) = extract_usage_from_chunk(&sse_chunk) {
                        final_usage = Some(JsonUsage {
                            input_tokens: usage.prompt_tokens.unwrap_or(0),
                            output_tokens: usage.completion_tokens.unwrap_or(0),
                            total_tokens: usage.total_tokens.unwrap_or(0),
                        });
                    }

                    let (content, finish_reason) = extract_content_from_chunk(&sse_chunk);
                    if content.is_some() || finish_reason.is_some() {
                        if let Some(ref c) = content {
                            full_content.push_str(c);
                        }

                        if is_json {
                            let evt = StreamEvent::delta(seq, content, finish_reason);
                            println!("{}", serde_json::to_string(&evt).unwrap());
                        } else if !quiet {
                            if let Some(ref c) = content {
                                let _ = write!(std::io::stdout(), "{}", c);
                                let _ = std::io::stdout().flush();
                            }
                        } else if let Some(ref c) = content {
                            full_content.push_str(c);
                        }
                        seq += 1;
                    }
                }
                None => {
                    // Non-data lines (comments, empty lines) — skip
                }
            }
        }
    }

    // If we exit the loop without [DONE], still emit done for JSON mode
    if is_json {
        if let Some(ref usage) = final_usage {
            let usage_evt = StreamEvent::usage_event(seq, usage.clone());
            println!("{}", serde_json::to_string(&usage_evt).unwrap());
            seq += 1;
        }
        let done_evt = StreamEvent::done(seq);
        println!("{}", serde_json::to_string(&done_evt).unwrap());
    } else {
        println!();
        if let Some(ref out_path) = args.out {
            write_output_file(out_path, &full_content)?;
        }
    }

    Ok(())
}

fn resolve_effective_usage_mode(usage_mode: Option<&str>, is_json: bool) -> &str {
    match usage_mode {
        Some(mode) => mode,
        None => {
            if is_json {
                "json"
            } else {
                "stderr"
            }
        }
    }
}

fn write_output_file(path: &str, content: &str) -> Result<()> {
    if let Some(parent) = std::path::Path::new(path).parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent).map_err(|e| {
                let llm_err = LlmError::OutputWrite {
                    path: path.to_string(),
                    reason: e.to_string(),
                };
                anyhow::anyhow!("{}", llm_err)
            })?;
        }
    }
    std::fs::write(path, content).map_err(|e| {
        let llm_err = LlmError::OutputWrite {
            path: path.to_string(),
            reason: e.to_string(),
        };
        anyhow::anyhow!("{}", llm_err)
    })
}
