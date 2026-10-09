//! Literal executable/argv transport for command-string hook providers.
use super::{HookCommand, IntegrationError};

pub(super) fn command(handler: &HookCommand) -> Result<String, IntegrationError> {
    command_for(handler, cfg!(windows))
}

fn command_for(handler: &HookCommand, windows: bool) -> Result<String, IntegrationError> {
    let executable = handler
        .executable
        .to_str()
        .ok_or_else(|| IntegrationError::Invalid("hook executable must be UTF-8".into()))?;
    if windows {
        use base64::Engine;
        // Hook providers execute command strings through a native shell.
        // Keep the outer command shell-neutral. PowerShell only interprets an
        // encoded constant script; ProcessStartInfo passes native argv/stdin and
        // stdout without PowerShell's lossy native argument or pipeline conversion.
        let argv = handler
            .arguments
            .iter()
            .map(|v| windows_argument(v))
            .collect::<Vec<_>>()
            .join(" ");
        // StandardInput creates a StreamWriter even though we copy into its
        // BaseStream. Its default encoding can emit a BOM when initialized or
        // closed, depending on the host console encoding. Disable that preamble
        // explicitly; payload bytes (including any actual BOM) remain untouched.
        // Windows PowerShell's .NET Framework lacks StandardInputEncoding, so
        // Process.Start captures a BOM-free UTF-8 Console.InputEncoding instead.
        // The code page stays 65001 and the original encoding is immediately
        // restored after construction, including if process creation fails.
        let script = format!("$ErrorActionPreference='Stop';$ProgressPreference='SilentlyContinue';$p=[System.Diagnostics.ProcessStartInfo]::new();$p.FileName={};$p.Arguments={};$p.UseShellExecute=$false;$p.RedirectStandardInput=$true;$p.RedirectStandardOutput=$true;$p.RedirectStandardError=$true;$encoding=[Console]::InputEncoding;if($encoding.CodePage -eq 65001){{[Console]::InputEncoding=[System.Text.UTF8Encoding]::new($false)}};try{{$c=[System.Diagnostics.Process]::Start($p)}}finally{{[Console]::InputEncoding=$encoding}};$o=$c.StandardOutput.BaseStream.CopyToAsync([Console]::OpenStandardOutput());$e=$c.StandardError.BaseStream.CopyToAsync([Console]::OpenStandardError());[Console]::OpenStandardInput().CopyTo($c.StandardInput.BaseStream);$c.StandardInput.Close();$c.WaitForExit();[void]$o.GetAwaiter().GetResult();[void]$e.GetAwaiter().GetResult();exit $c.ExitCode", ps_literal(executable), ps_literal(&argv));
        let bytes: Vec<u8> = script.encode_utf16().flat_map(u16::to_le_bytes).collect();
        let encoded = base64::engine::general_purpose::STANDARD.encode(bytes);
        let command =
            format!("powershell.exe -NoProfile -NonInteractive -EncodedCommand {encoded}");
        // Leave room for the provider's surrounding stdin transport and shell command.
        if command.len() > 24000 {
            return Err(IntegrationError::Invalid(
                "Windows hook command exceeds supported length".into(),
            ));
        }
        Ok(command)
    } else {
        Ok(std::iter::once(executable)
            .chain(handler.arguments.iter().map(String::as_str))
            .map(|v| format!("'{}'", v.replace('\'', "'\\''")))
            .collect::<Vec<_>>()
            .join(" "))
    }
}

fn ps_literal(value: &str) -> String {
    format!("'{}'", value.replace('\'', "''"))
}

fn windows_argument(value: &str) -> String {
    let mut result = String::from("\"");
    let mut slashes = 0;
    for c in value.chars() {
        if c == '\\' {
            slashes += 1;
            continue;
        }
        result.extend(std::iter::repeat('\\').take(if c == '"' {
            slashes * 2 + 1
        } else {
            slashes
        }));
        result.push(c);
        slashes = 0;
    }
    result.extend(std::iter::repeat('\\').take(slashes * 2));
    result.push('"');
    result
}

#[cfg(test)]
#[path = "command_tests.rs"]
mod tests;
