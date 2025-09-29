# SYS-101-00010-Shell-Taha-Keler
## vssh – Very Simple SHell

**Author:** Taha Keler  
**Course:** SYS-101  
**Assessment:** Unix Shell – Assessment 1  
**Repository:** https://github.com/tahakeler/SYS-101-00010-Shell-Taha-Keler

---

## Overview
This project implements a miniature Unix-like shell called **vssh** (Very Simple SHell) in Rust.  
It was developed for SYS-101 Assessment 1 and fulfills the three assignment levels:  
1. Core shell features with process forking.  
2. Input/Output redirection.  
3. Command pipelines.  

---

## Features

### Level 1 – Core Shell
- Prompt displays the current working directory.
- Built-in commands:  
  - `exit` – terminates the shell.  
  - `cd [dir]` – changes current directory.  
- Executes external commands with `fork` + `execvp`.  
- Supports background processes using `&` (prints PID).  
- Ignores blank lines and handles errors gracefully.  

### Level 2 – I/O Redirection
- `< file` – input redirection.  
- `> file` – output redirection (creates/truncates file).  
- Can combine both: `sort < in.txt > out.txt`.  
- Handles missing files or permission errors safely.  

### Level 3 – Pipelines
- `|` – connects commands into pipelines.  
- Multi-stage pipelines supported.  
- Input redirection allowed on the first command, output redirection on the last.  
- Pipelines can also run in background with `&`.  

---

## Build & Run

Make sure Rust is installed. From the project root:

```bash
cargo build
cargo run --bin vssh
````

## Example Usage
```bash
pwd
echo hello > out.txt
cat < out.txt
ls -l | grep Cargo | sort
cat < out.txt | tr a-z A-Z > upper.txt
sleep 5 &
exit
```

## Project Structure
```css
.
├── Cargo.toml
├── README.md
└── src/
    └── bin/
        └── vssh.rs   # Main shell implementation
```
