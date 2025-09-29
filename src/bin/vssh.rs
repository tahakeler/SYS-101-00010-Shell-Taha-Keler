use anyhow::{anyhow, Context, Result};
use nix::fcntl::{open, OFlag};
use nix::sys::stat::Mode;
use nix::sys::wait::{waitpid, WaitStatus};
use nix::unistd::{dup2, execvp, fork, ForkResult};
use std::env;
use std::ffi::CString;
use std::io::{self, Write};
use std::path::Path;

struct Parsed {
    background: bool,
    input: Option<String>,
    output: Option<String>,
    argv: Vec<CString>,
}

fn main() -> Result<()> {
    loop {
        let cwd = env::current_dir().map(|p| p.display().to_string()).unwrap_or_else(|_| "?".to_string());
        print!("{}$ ", cwd);
        io::stdout().flush().ok();
        let mut line = String::new();
        if io::stdin().read_line(&mut line)? == 0 { eprintln!(); break; }
        let line = line.trim().to_string();
        if line.is_empty() { continue; }
        if line == "exit" { break; }
        if line.starts_with("cd") { if let Err(e) = builtin_cd(&line) { eprintln!("{e}"); } continue; }
        let parsed = match parse_line(&line) { Ok(p) => p, Err(e) => { eprintln!("parse error: {e}"); continue; } };
        if parsed.argv.is_empty() { continue; }
        if let Err(e) = run_command(parsed) { eprintln!("error: {e}"); }
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

fn run_command(p: Parsed) -> Result<()> {
    match unsafe { fork()? } {
        ForkResult::Child => {
            if let Some(ref infile) = p.input {
                let fd = open(Path::new(infile), OFlag::O_RDONLY, Mode::from_bits_truncate(0o644)).with_context(|| format!("cannot open for input: {infile}"))?;
                dup2(fd, 0).ok();
            }
            if let Some(ref outfile) = p.output {
                let fd = open(Path::new(outfile), OFlag::O_CREAT | OFlag::O_WRONLY | OFlag::O_TRUNC, Mode::from_bits_truncate(0o644)).with_context(|| format!("cannot open for output: {outfile}"))?;
                dup2(fd, 1).ok();
            }
            let err = execvp(&p.argv[0], &p.argv).err().unwrap();
            eprintln!("exec failed: {err}");
            std::process::exit(127);
        }
        ForkResult::Parent { child } => {
            if p.background {
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

fn parse_line(line: &str) -> Result<Parsed> {
    let mut s = line.trim().to_string();
    let mut background = false;
    if s.ends_with('&') { background = true; s.pop(); s = s.trim_end().to_string(); }
    let toks = shell_split(&s);
    let mut argv: Vec<CString> = Vec::new();
    let mut input: Option<String> = None;
    let mut output: Option<String> = None;
    let mut i = 0;
    while i < toks.len() {
        match toks[i].as_str() {
            "<" => {
                if i + 1 >= toks.len() { return Err(anyhow!("missing input filename")); }
                input = Some(toks[i + 1].clone());
                i += 2;
            }
            ">" => {
                if i + 1 >= toks.len() { return Err(anyhow!("missing output filename")); }
                output = Some(toks[i + 1].clone());
                i += 2;
            }
            "&" => { i += 1; }
            other => {
                argv.push(CString::new(other).map_err(|_| anyhow!("NUL in arg"))?);
                i += 1;
            }
        }
    }
    Ok(Parsed { background, input, output, argv })
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
