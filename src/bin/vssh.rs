use anyhow::{anyhow, Context, Result};
use nix::fcntl::{open, OFlag};
use nix::sys::stat::Mode;
use nix::sys::wait::{waitpid, WaitStatus};
use nix::unistd::{dup2, execvp, fork, pipe, ForkResult, Pid};
use std::env;
use std::ffi::CString;
use std::io::{self, Write};
use std::os::unix::io::{AsRawFd, OwnedFd};
use std::path::Path;

#[derive(Clone)]
struct ParsedLine {
    background: bool,
    input: Option<String>,
    output: Option<String>,
    pipeline: Vec<Vec<CString>>,
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
        if let Err(e) = execute(parsed) { eprintln!("error: {e}"); }
    }
    Ok(())
}

fn builtin_cd(line: &str) -> Result<()> {
    let parts = shell_split(line);
    if parts.len() == 1 {
        let home = env::var("HOME").map_err(|_| anyhow!("HOME not set"))?;
        env::set_current_dir(Path::new(&home)).with_context(|| "failed to change directory to HOME")?;
    } else {
        let target = &parts[1];
        env::set_current_dir(Path::new(target)).with_context(|| format!("cd: no such file or directory: {target}"))?;
    }
    Ok(())
}

fn execute(pl: ParsedLine) -> Result<()> {
    if pl.pipeline.len() == 1 { return exec_single(pl); }
    exec_pipeline(pl)
}

fn exec_single(pl: ParsedLine) -> Result<()> {
    let argv = &pl.pipeline[0];
    match unsafe { fork()? } {
        ForkResult::Child => {
            if let Some(ref infile) = pl.input {
                let fd = open(Path::new(infile), OFlag::O_RDONLY, Mode::from_bits_truncate(0o644)).with_context(|| format!("cannot open for input: {infile}"))?;
                dup2(fd, 0).ok();
            }
            if let Some(ref outfile) = pl.output {
                let fd = open(Path::new(outfile), OFlag::O_CREAT | OFlag::O_WRONLY | OFlag::O_TRUNC, Mode::from_bits_truncate(0o644)).with_context(|| format!("cannot open for output: {outfile}"))?;
                dup2(fd, 1).ok();
            }
            let err = execvp(&argv[0], &argv).err().unwrap();
            eprintln!("exec failed: {err}");
            std::process::exit(127);
        }
        ForkResult::Parent { child } => {
            if pl.background {
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

fn exec_pipeline(pl: ParsedLine) -> Result<()> {
    let n = pl.pipeline.len();
    let mut pids: Vec<Pid> = Vec::with_capacity(n);
    let mut prev_read_end: Option<OwnedFd> = None;

    for i in 0..n {
        let (read_end, write_end): (Option<OwnedFd>, Option<OwnedFd>) =
            if i < n - 1 { let (r, w) = pipe()?; (Some(r), Some(w)) } else { (None, None) };

        match unsafe { fork()? } {
            ForkResult::Child => {
                if i == 0 {
                    if let Some(ref infile) = pl.input {
                        let fd = open(Path::new(infile), OFlag::O_RDONLY, Mode::from_bits_truncate(0o644)).with_context(|| format!("cannot open for input: {infile}"))?;
                        dup2(fd, 0).ok();
                    }
                }
                if let Some(ref prev_r) = prev_read_end { dup2(prev_r.as_raw_fd(), 0).ok(); }
                if let Some(ref w) = write_end {
                    dup2(w.as_raw_fd(), 1).ok();
                } else if let Some(ref outfile) = pl.output {
                    let fd = open(Path::new(outfile), OFlag::O_CREAT | OFlag::O_WRONLY | OFlag::O_TRUNC, Mode::from_bits_truncate(0o644)).with_context(|| format!("cannot open for output: {outfile}"))?;
                    dup2(fd, 1).ok();
                }
                drop(prev_read_end);
                drop(read_end);
                drop(write_end);
                let argv = &pl.pipeline[i];
                let err = execvp(&argv[0], &argv).err().unwrap();
                eprintln!("exec failed: {err}");
                std::process::exit(127);
            }
            ForkResult::Parent { child } => {
                pids.push(child);
                drop(prev_read_end);
                if let Some(w) = write_end { drop(w); }
                prev_read_end = read_end;
            }
        }
    }

    if pl.background {
        if let Some(first) = pids.first() { println!("Starting background process {}", first.as_raw()); }
        return Ok(());
    }

    for pid in pids {
        loop {
            match waitpid(pid, None) {
                Ok(WaitStatus::Exited(_, _)) | Ok(WaitStatus::Signaled(_, _, _)) => break,
                Ok(_) => continue,
                Err(e) => return Err(anyhow!("waitpid failed: {e}")),
            }
        }
    }
    Ok(())
}

fn parse_line(line: &str) -> Result<ParsedLine> {
    let mut s = line.trim().to_string();
    let mut background = false;
    if s.ends_with('&') { background = true; s.pop(); s = s.trim_end().to_string(); }
    let mut segments: Vec<String> = s.split('|').map(|t| t.trim().to_string()).collect();
    if segments.is_empty() { return Err(anyhow!("empty command")); }
    let mut input: Option<String> = None;
    let mut output: Option<String> = None;

    {
        let seg = &segments[0];
        if seg.contains('<') {
            let parts: Vec<&str> = seg.split('<').collect();
            if parts.len() != 2 { return Err(anyhow!("invalid input redirection")); }
            let cmd = parts[0].trim();
            let file = parts[1].trim();
            if file.is_empty() { return Err(anyhow!("missing input filename")); }
            input = Some(file.to_string());
            segments[0] = cmd.to_string();
        }
    }
    let last = segments.len() - 1;
    {
        let seg = &segments[last];
        if seg.contains('>') {
            let parts: Vec<&str> = seg.split('>').collect();
            if parts.len() != 2 { return Err(anyhow!("invalid output redirection")); }
            let cmd = parts[0].trim();
            let file = parts[1].trim();
            if file.is_empty() { return Err(anyhow!("missing output filename")); }
            output = Some(file.to_string());
            segments[last] = cmd.to_string();
        }
    }

    let mut pipeline: Vec<Vec<CString>> = Vec::new();
    for seg in segments {
        let tokens = shell_split(&seg);
        if tokens.is_empty() { return Err(anyhow!("empty pipeline segment")); }
        let argv: Vec<CString> = tokens.into_iter().map(|t| CString::new(t).map_err(|_| anyhow!("NUL in arg"))).collect::<Result<Vec<_>>>()?;
        pipeline.push(argv);
    }

    Ok(ParsedLine { background, input, output, pipeline })
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
