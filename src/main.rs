use chrono::Local;
use clap::{Parser, Subcommand};
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

mod tui;

#[derive(Parser)]
#[command(name = "hm", about = "Minimal thought-capture CLI")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// List recent entries
    Ls {
        #[arg(short = 'n', long, default_value = "20")]
        count: usize,
    },
    /// Clone a repo and write config
    Init {
        #[arg(long)]
        repo: String,
    },
    /// Delete an entry by commit hash
    Delete { hash: String },
    /// Push commits to remote
    Push,
    /// Search entries
    Search { query: String },
    /// View a markdown file
    View { path: String },
    /// Open the interactive TUI
    Tui,
    /// Capture a thought (default when no subcommand given)
    #[command(external_subcommand)]
    Capture(Vec<String>),
}

pub(crate) enum Theme {
    Laptop,
    Eink,
}

pub(crate) struct Config {
    pub(crate) repo: PathBuf,
    pub(crate) file: String,
    pub(crate) remote: String,
    pub(crate) llm: bool,
    pub(crate) llm_model: String,
    pub(crate) theme: Theme,
}

const LLM_ENDPOINT: &str = "http://localhost:11434/api/generate";

// " — " (space, U+2014 em dash, space)
pub(crate) const SEP: &str = " \u{2014} ";

fn home() -> PathBuf {
    env::var("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("/tmp"))
}

fn expand_tilde(s: &str) -> PathBuf {
    if s == "~" {
        home()
    } else if s.starts_with("~/") {
        home().join(&s[2..])
    } else {
        PathBuf::from(s)
    }
}

fn config_path() -> PathBuf {
    home().join(".config/hm/config.toml")
}

pub(crate) fn load_config() -> Result<Config, String> {
    let path = config_path();
    let content = fs::read_to_string(&path).map_err(|_| {
        format!(
            "Config not found at {}. Run `hm init --repo <url>` first.",
            path.display()
        )
    })?;

    let mut repo = String::new();
    let mut file = String::new();
    let mut remote = String::new();
    let mut llm = false;
    let mut llm_model = String::new();
    let mut theme = String::new();

    for line in content.lines() {
        let line = line.trim();
        if line.starts_with('#') || line.is_empty() {
            continue;
        }
        if let Some((k, v)) = line.split_once('=') {
            let v = v.trim().trim_matches('"');
            match k.trim() {
                "repo" => repo = v.to_string(),
                "file" => file = v.to_string(),
                "remote" => remote = v.to_string(),
                "llm" => llm = v == "true",
                "llm_model" => llm_model = v.to_string(),
                "theme" => theme = v.to_string(),
                _ => {}
            }
        }
    }

    if repo.is_empty() {
        return Err("Config missing 'repo' field.".to_string());
    }

    Ok(Config {
        repo: expand_tilde(&repo),
        file: if file.is_empty() {
            "hm.md".to_string()
        } else {
            file
        },
        remote,
        llm,
        llm_model: if llm_model.is_empty() { "llama3.2".to_string() } else { llm_model },
        theme: if theme == "eink" { Theme::Eink } else { Theme::Laptop },
    })
}

pub(crate) fn hm_path(config: &Config) -> PathBuf {
    config.repo.join(&config.file)
}

pub(crate) fn git_silent(repo: &Path, args: &[&str]) -> Result<(), String> {
    let out = Command::new("git")
        .current_dir(repo)
        .args(args)
        .output()
        .map_err(|e| format!("Failed to run git: {}", e))?;
    if out.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&out.stderr).trim().to_string())
    }
}

fn git_passthrough(repo: &Path, args: &[&str]) -> Result<(), String> {
    let status = Command::new("git")
        .current_dir(repo)
        .args(args)
        .status()
        .map_err(|e| format!("Failed to run git: {}", e))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!(
            "git {} failed",
            args.first().copied().unwrap_or("unknown")
        ))
    }
}

fn preview(s: &str) -> String {
    const N: usize = 10;
    let chars: Vec<char> = s.chars().collect();
    if chars.len() <= N {
        s.to_string()
    } else {
        format!("{}...", chars[..N].iter().collect::<String>())
    }
}

fn is_capture_subject(s: &str) -> bool {
    // New format: "capture: <text>"
    if s.starts_with("capture: ") {
        return true;
    }
    // Legacy format: bare "YYYY-MM-DD HH:MM"
    let b = s.as_bytes();
    s.len() == 16
        && b[4] == b'-'
        && b[7] == b'-'
        && b[10] == b' '
        && b[13] == b':'
        && b[..4].iter().all(|c| c.is_ascii_digit())
        && b[5..7].iter().all(|c| c.is_ascii_digit())
        && b[8..10].iter().all(|c| c.is_ascii_digit())
        && b[11..13].iter().all(|c| c.is_ascii_digit())
        && b[14..16].iter().all(|c| c.is_ascii_digit())
}

// Returns short commit hashes for capture commits, newest first.
pub(crate) fn capture_hashes(config: &Config) -> Vec<String> {
    let Ok(out) = Command::new("git")
        .current_dir(&config.repo)
        .args(["log", "--format=%h %s", "--", &config.file])
        .output()
    else {
        return vec![];
    };
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter_map(|line| {
            let (hash, subject) = line.split_once(' ')?;
            is_capture_subject(subject).then(|| hash.to_string())
        })
        .collect()
}

// --- Core logic used by both CLI and TUI ---

fn llm_reply(text: &str, model: &str) -> Option<String> {
    if !text.contains('?') {
        return None;
    }
    let body = serde_json::json!({
        "model": model,
        "prompt": format!("Answer in one concise sentence, no preamble: {}", text),
        "stream": false
    });
    let agent = ureq::AgentBuilder::new()
        .timeout(std::time::Duration::from_secs(25))
        .build();
    let resp = agent
        .post(LLM_ENDPOINT)
        .set("Content-Type", "application/json")
        .send_string(&body.to_string())
        .ok()?;
    let resp_body = resp.into_string().ok()?;
    let json: serde_json::Value = serde_json::from_str(&resp_body).ok()?;
    let reply = json["response"].as_str()?.trim().to_string();
    if reply.is_empty() { None } else { Some(reply) }
}

pub(crate) fn do_capture(text: &str, config: &Config) -> Result<(String, String, Option<String>), String> {
    let path = hm_path(config);
    let ts = Local::now().format("%Y-%m-%d %H:%M").to_string();
    let entry = format!("{}{}{}\n", ts, SEP, text);
    let existing = fs::read_to_string(&path).unwrap_or_default();
    fs::write(&path, format!("{}{}", entry, existing))
        .map_err(|e| format!("Failed to write {}: {}", path.display(), e))?;
    git_silent(&config.repo, &["add", &config.file])?;
    git_silent(&config.repo, &["commit", "-m", &format!("capture: {}", preview(text))])?;
    let hash = Command::new("git")
        .current_dir(&config.repo)
        .args(["rev-parse", "--short", "HEAD"])
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|| "???????".to_string());
    let reply = if config.llm { llm_reply(text, &config.llm_model) } else { None };
    Ok((hash, ts, reply))
}

pub(crate) fn do_delete(hash: &str, config: &Config) -> Result<(), String> {
    let hashes = capture_hashes(config);
    let idx = hashes
        .iter()
        .position(|h| h.starts_with(hash) || hash.starts_with(h.as_str()))
        .ok_or_else(|| format!("No entry found for '{}'.", hash))?;

    let path = hm_path(config);
    let content = fs::read_to_string(&path).map_err(|_| "No entries found.".to_string())?;

    let lines: Vec<&str> = content
        .lines()
        .filter(|l| !l.trim().is_empty())
        .collect();

    if idx >= lines.len() {
        return Err("Entry not found in file.".to_string());
    }

    let entry_text = match lines[idx].find(SEP) {
        Some(pos) => &lines[idx][pos + SEP.len()..],
        None => lines[idx],
    };
    let msg = format!("delete: {}", preview(entry_text));

    let new_content = lines
        .iter()
        .enumerate()
        .filter(|(i, _)| *i != idx)
        .map(|(_, l)| *l)
        .collect::<Vec<_>>()
        .join("\n")
        + "\n";

    fs::write(&path, new_content).map_err(|e| format!("Failed to write file: {}", e))?;

    git_silent(&config.repo, &["add", &config.file])?;
    git_silent(&config.repo, &["commit", "-m", &msg])?;

    Ok(())
}

pub(crate) fn do_push(config: &Config) -> Result<(), String> {
    if git_silent(&config.repo, &["push"]).is_err() {
        git_silent(&config.repo, &["push", "-u", "origin", "HEAD"])?;
    }
    Ok(())
}

// --- CLI command handlers ---

fn cmd_capture(parts: &[String]) -> Result<(), String> {
    if parts.is_empty() {
        return Err("Provide a thought to capture.".to_string());
    }
    let text = parts.join(" ");
    let text = text.trim();
    let config = load_config()?;
    let (hash, ts, reply) = do_capture(text, &config)?;
    println!("{}  {}{}{}", hash, ts, SEP, text);
    if let Some(r) = reply {
        println!("  \u{2192} {}", r);
    }
    Ok(())
}

fn cmd_ls(count: usize) -> Result<(), String> {
    let config = load_config()?;
    let path = hm_path(&config);

    let content = match fs::read_to_string(&path) {
        Ok(c) => c,
        Err(_) => {
            println!("No entries yet.");
            return Ok(());
        }
    };

    let entries: Vec<&str> = content
        .lines()
        .filter(|l| !l.trim().is_empty())
        .take(count)
        .collect();

    if entries.is_empty() {
        println!("No entries yet.");
        return Ok(());
    }

    let hashes = capture_hashes(&config);

    for (i, line) in entries.iter().enumerate() {
        let hash = hashes.get(i).map(|s| s.as_str()).unwrap_or("???????");
        if let Some(pos) = line.find(SEP) {
            let ts = &line[..pos];
            let text = &line[pos + SEP.len()..];
            println!("{}  {}  {}", hash, ts, text);
        } else {
            println!("{}  {}", hash, line);
        }
    }

    Ok(())
}

fn cmd_init(repo_url: &str) -> Result<(), String> {
    let data_dir = home().join(".local/share/hm");

    if data_dir.exists() {
        return Err(format!(
            "Directory {} already exists. Remove it to reinitialize.",
            data_dir.display()
        ));
    }

    println!("Cloning {}...", repo_url);
    let clone_status = Command::new("git")
        .args(["clone", repo_url])
        .arg(&data_dir)
        .status()
        .map_err(|e| format!("Failed to run git clone: {}", e))?;

    if !data_dir.join(".git").exists() {
        if !clone_status.success() {
            fs::create_dir_all(&data_dir)
                .map_err(|e| format!("Failed to create data dir: {}", e))?;
            git_silent(&data_dir, &["init"])?;
            git_silent(&data_dir, &["remote", "add", "origin", repo_url])?;
        } else {
            return Err(format!(
                "Clone succeeded but .git missing in {}",
                data_dir.display()
            ));
        }
    }

    let hm = data_dir.join("hm.md");
    if !hm.exists() {
        fs::write(&hm, "").map_err(|e| format!("Failed to create hm.md: {}", e))?;
        git_silent(&data_dir, &["add", "hm.md"])?;
        git_silent(&data_dir, &["commit", "-m", "init: create hm.md"])?;
        let _ = git_passthrough(&data_dir, &["push", "-u", "origin", "HEAD"]);
    }

    let config_dir = home().join(".config/hm");
    fs::create_dir_all(&config_dir)
        .map_err(|e| format!("Failed to create config dir: {}", e))?;

    let config_content = format!(
        "repo = \"{}\"\nfile = \"hm.md\"\nremote = \"{}\"\n",
        data_dir.display(),
        repo_url
    );
    fs::write(config_dir.join("config.toml"), &config_content)
        .map_err(|e| format!("Failed to write config: {}", e))?;

    println!(
        "Initialized. Config at {}.",
        config_dir.join("config.toml").display()
    );
    Ok(())
}

fn cmd_search(query: &str) -> Result<(), String> {
    let config = load_config()?;
    let path = hm_path(&config);
    let content = fs::read_to_string(&path).unwrap_or_default();
    let query_lower = query.to_lowercase();
    let hashes = capture_hashes(&config);

    let matches: Vec<(usize, &str)> = content
        .lines()
        .filter(|l| !l.trim().is_empty())
        .enumerate()
        .filter(|(_, l)| l.to_lowercase().contains(&query_lower))
        .collect();

    if matches.is_empty() {
        println!("No matches.");
        return Ok(());
    }

    for (i, line) in matches {
        let hash = hashes.get(i).map(|s| s.as_str()).unwrap_or("???????");
        if let Some(pos) = line.find(SEP) {
            let ts = &line[..pos];
            let text = &line[pos + SEP.len()..];
            println!("{}  {}  {}", hash, ts, text);
        } else {
            println!("{}  {}", hash, line);
        }
    }

    Ok(())
}

fn cmd_view(path: &str) -> Result<(), String> {
    if !path.ends_with(".md") {
        return Err(format!("'{}' is not a markdown file — hm view only supports .md files", path));
    }
    let config = load_config()?;
    let expanded = expand_tilde(path);
    let content = fs::read_to_string(&expanded)
        .map_err(|e| format!("Cannot read '{}': {}", expanded.display(), e))?;
    let mut skin = termimad::MadSkin::default();
    if let Theme::Eink = config.theme {
        skin.set_headers_fg(termimad::crossterm::style::Color::Reset);
        skin.bold.set_fg(termimad::crossterm::style::Color::Reset);
        skin.italic.set_fg(termimad::crossterm::style::Color::Reset);
        skin.inline_code.set_fg(termimad::crossterm::style::Color::Reset);
    }
    skin.print_text(&content);
    Ok(())
}

fn cmd_delete(hash: &str) -> Result<(), String> {
    let config = load_config()?;
    do_delete(hash, &config)?;
    println!("Deleted {}.", hash);
    Ok(())
}

fn cmd_push() -> Result<(), String> {
    let config = load_config()?;
    do_push(&config)?;
    println!("Pushed → {}", config.remote);
    Ok(())
}

fn main() {
    let cli = Cli::parse();
    let result = match cli.command {
        Commands::Ls { count } => cmd_ls(count),
        Commands::Delete { hash } => cmd_delete(&hash),
        Commands::Init { repo } => cmd_init(&repo),
        Commands::Push => cmd_push(),
        Commands::Search { query } => cmd_search(&query),
        Commands::View { path } => cmd_view(&path),
        Commands::Tui => tui::run(),
        Commands::Capture(parts) => cmd_capture(&parts),
    };

    if let Err(e) = result {
        eprintln!("error: {}", e);
        std::process::exit(1);
    }
}
