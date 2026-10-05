// Standalone std-only peer compiled by the ACP integration test. No model calls.
use std::io::{self,BufRead,Write};
fn send(value:&str){println!("{value}");io::stdout().flush().unwrap();}
fn result(id:&str,body:&str){send(&format!("{{\"jsonrpc\":\"2.0\",\"id\":{id},\"result\":{body}}}"));}
fn main(){
 if std::env::args().any(|a|a=="no-read") {std::thread::sleep(std::time::Duration::from_secs(60));return;}
 let mut prompt=None;
 for line in io::stdin().lock().lines(){let line=line.unwrap();let id=line.split("\"id\":").nth(1).map(|s|s.chars().take_while(|c|c.is_ascii_digit()).collect::<String>()).unwrap_or_default();
  if let Ok(path)=std::env::var("AIKIT_FIXTURE_AUDIT") { if line.contains("\"method\":") { let mut f=std::fs::OpenOptions::new().create(true).append(true).open(path).unwrap(); writeln!(f,"{line}").unwrap(); } }
  if line.contains("\"method\":\"initialize\"") && std::env::var_os("AIKIT_FIXTURE_STALL_INIT").is_some(){continue;}
  if line.contains("\"method\":\"initialize\""){result(&id,r#"{"protocolVersion":1,"agentCapabilities":{"loadSession":true}}"#);}
  else if line.contains("\"method\":\"session/new\""){result(&id,r#"{"sessionId":"fixture-session"}"#);}
  else if line.contains("\"method\":\"session/load\"")||line.contains("\"method\":\"session/set_model\""){result(&id,"{}");}
  else if line.contains("\"method\":\"session/prompt\""){prompt=Some(id);send(r#"{"jsonrpc":"2.0","id":900,"method":"session/request_permission","params":{"sessionId":"fixture-session","toolCall":{"toolCallId":"read-1"},"options":[{"optionId":"native-once","kind":"allow_once"}]}}"#);}
  else if line.contains("\"method\":\"session/cancel\""){if let Some(id)=prompt.take(){result(&id,r#"{"stopReason":"cancelled"}"#);}}
  else if id=="900" {if let Some(id)=prompt.take(){send(r#"{"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"fixture-session","update":{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"fixture response"}}}}"#);result(&id,r#"{"stopReason":"end_turn"}"#);}}
 }
}
