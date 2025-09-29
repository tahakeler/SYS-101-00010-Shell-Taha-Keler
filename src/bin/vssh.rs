use anyhow::{anyhow, Context, Result};
use nix::sys::wait::{waitpid, WaitStatus};
use nix::unistd::{execvp, fork, ForkResult};
use std::env;
use std::ffi::CString;
use std::io::{self, Write};
use std::path::Path;

fn main() -> Result<()> {
    loop {
        let cwd = env::current_dir().map(|p| p.display().to_string()).unwrap_or_else(|_| "?".to_string());
        print!("{}$ ", cwd);
        io::stdout().flush().ok();
        let mut line = String::new();
        let n = io::stdin().read_line(&mut line)?;
        if n == 0 { eprintln!(); break; }
        let mut line = line.trim().to_string();
        if line.is_empty() { continue; }
        if line == "exit" { break; }
        if line.starts_with("cd") { if let Err(e) = builtin_cd(&line) { eprintln!("{e}"); } continue; }
        let mut background = false;
        if line.ends_with('&') { background = true; line.pop(); line = line.trim_end().to_string(); }
        if let Err(e) = run_external(&line, background) { eprintln!("error: {e}"); }
    }
    Ok(())
}

fn builtin_cd(line: &str) -> Result<()> {
    let parts = shell_split(line);
    match parts.len() {
        1 => {
            let home = env::var("HOME").map_err(|_| anyhow!("HOME not set"))?;
            env::set_current_dir(Path::new(&home)).with_context(|| "failed to change directory to HOME")?;
        }
        _ => {
            let target = &parts[1];
            env::set_current_dir(Path::new(target)).with_context(|| format!("cd: no such file or directory: {target}"))?;
        }
    }
    Ok(())
}

fn run_external(command: &str, background: bool) -> Result<()> {
    let argv = externalize(command)?;
    if argv.is_empty() { return Ok(()); }
    match unsafe { fork()? } {
        ForkResult::Child => {
            let err = execvp(&argv[0], &argv).err().unwrap();
            eprintln!("exec failed: {err}");
            std::process::exit(127);
        }
        ForkResult::Parent { child } => {
            if background {
                println!("Starting background process {}", child.as_raw());
                return Ok(());
            } else {
                loop {
                    match waitpid(child, None) {
                        Ok(WaitStatus::Exited(_, _)) | Ok(WaitStatus::Signaled(_, _, _)) => break,
                        Ok(_) => continue,
                        Err(e) => return Err(anyhow!("waitpid failed: {e}")),
                    }
                }
            }
        }
    }
    Ok(())
}

fn shell_split(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut buf = String::new();
    let mut in_quotes = false;
    let mut quote_char = ' ';
    for c in s.chars() {
        match c {
            '\'' | '"' => {
                if in_quotes && c == quote_char { in_quotes = false; }
                else if !in_quotes { in_quotes = true; quote_char = c; }
                else { buf.push(c); }
            }
            ' ' | '\t' if !in_quotes => {
                if !buf.is_empty() { out.push(buf.clone()); buf.clear(); }
            }
            _ => buf.push(c),
        }
    }
    if !buf.is_empty() { out.push(buf); }
    out
}

fn externalize(command: &str) -> Result<Vec<CString>> {
    let parts = shell_split(command);
    let mut v = Vec::with_capacity(parts.len());
    for s in parts { v.push(CString::new(s).map_err(|_| anyhow!("NUL byte in argument"))?); }
    Ok(v)
}
