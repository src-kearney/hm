use chrono::Local;
use clap::{Parser, Subcommand};
use std::env;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, atomic::{AtomicBool, Ordering}};

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
    /// Show commit history of the notes file
    Log {
        #[arg(short = 'n', long, default_value = "20")]
        count: usize,
    },
    /// Pull latest changes from remote
    Pull,
    /// Create a new encrypted draft post
    Write {
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        title: Vec<String>,
    },
    /// Draft management
    Draft {
        #[command(subcommand)]
        cmd: DraftCmd,
    },
    /// Publish a draft to the blog repo
    Publish { slug: String },
    /// Blog repo management
    Blog {
        #[command(subcommand)]
        cmd: BlogCmd,
    },
    /// View or set config values
    Config {
        #[command(subcommand)]
        cmd: ConfigCmd,
    },
    /// Capture a thought (default when no subcommand given)
    #[command(external_subcommand)]
    Capture(Vec<String>),
}

#[derive(Subcommand)]
enum ConfigCmd {
    /// List all config values (including defaults)
    Ls,
    /// Set a config value
    Set { key: String, value: String },
}

#[derive(Subcommand)]
enum DraftCmd {
    /// List all drafts
    Ls,
}

#[derive(Subcommand)]
enum BlogCmd {
    /// List published posts
    Ls,
    /// Push blog repo to remote
    Push,
    /// Move a published post back to drafts
    Demote { slug: String },
}

pub(crate) enum Theme {
    Laptop,
    Eink,
}

#[derive(Clone)]
pub(crate) enum LlmTrigger {
    Off,
    Heuristic,
    Classifier,
    Always,
}

pub(crate) struct Config {
    pub(crate) repo: PathBuf,
    pub(crate) file: String,
    pub(crate) remote: String,
    pub(crate) llm: bool,
    pub(crate) llm_model: String,
    pub(crate) llm_trigger: LlmTrigger,
    pub(crate) llm_classifier_prompt: String,
    pub(crate) theme: Theme,
    pub(crate) blog_repo: Option<PathBuf>,
    pub(crate) age_key: Option<PathBuf>,
}

const LLM_ENDPOINT: &str = "http://localhost:11434/api/generate";
const DEFAULT_CLASSIFIER_PROMPT: &str =
    "Does this thought warrant a brief reply? Say 'yes' for questions or thoughts that invite a response, 'no' for plain statements or observations. Answer only 'yes' or 'no': {thought}";

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
    let mut llm_trigger = String::new();
    let mut llm_classifier_prompt = String::new();
    let mut theme = String::new();
    let mut blog_repo = String::new();
    let mut age_key = String::new();

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
                "llm_trigger" => llm_trigger = v.to_string(),
                "llm_classifier_prompt" => llm_classifier_prompt = v.to_string(),
                "theme" => theme = v.to_string(),
                "blog_repo" => blog_repo = v.to_string(),
                "age_key" => age_key = v.to_string(),
                _ => {}
            }
        }
    }

    if repo.is_empty() {
        return Err("Config missing 'repo' field.".to_string());
    }

    Ok(Config {
        repo: expand_tilde(&repo),
        file: if file.is_empty() { "hm.md".to_string() } else { file },
        remote,
        llm,
        llm_model: if llm_model.is_empty() { "llama3.2".to_string() } else { llm_model },
        llm_trigger: match llm_trigger.as_str() {
            "off"        => LlmTrigger::Off,
            "classifier" => LlmTrigger::Classifier,
            "always"     => LlmTrigger::Always,
            _            => LlmTrigger::Heuristic,
        },
        llm_classifier_prompt: if llm_classifier_prompt.is_empty() {
            DEFAULT_CLASSIFIER_PROMPT.to_string()
        } else {
            llm_classifier_prompt
        },
        theme: if theme == "eink" { Theme::Eink } else { Theme::Laptop },
        blog_repo: if blog_repo.is_empty() { None } else { Some(expand_tilde(&blog_repo)) },
        age_key: if age_key.is_empty() { None } else { Some(expand_tilde(&age_key)) },
    })
}

pub(crate) fn hm_path(config: &Config) -> PathBuf {
    config.repo.join(&config.file)
}

pub(crate) fn format_entry(ts: &str, text: &str) -> String {
    format!("## {}\n\n{}\n\n---\n", ts, text)
}

pub(crate) fn parse_entries(content: &str) -> Vec<(String, String)> {
    let mut entries = Vec::new();
    for block in content.split("\n---\n") {
        let block = block.trim_matches('\n');
        if block.is_empty() { continue; }
        if let Some(rest) = block.strip_prefix("## ") {
            let mut parts = rest.splitn(3, '\n');
            let ts = parts.next().unwrap_or("").trim().to_string();
            parts.next(); // blank line
            let text = parts.next().unwrap_or("").trim().to_string();
            if !ts.is_empty() {
                entries.push((ts, text));
            }
        } else {
            // legacy single-line format: "YYYY-MM-DD HH:MM — text"
            for line in block.lines() {
                let line = line.trim();
                if line.is_empty() { continue; }
                if let Some(pos) = line.find(SEP) {
                    entries.push((line[..pos].to_string(), line[pos + SEP.len()..].to_string()));
                } else {
                    entries.push((String::new(), line.to_string()));
                }
            }
        }
    }
    entries
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
    let chars: Vec<char> = s.chars().collect();
    if chars.len() <= 72 {
        s.to_string()
    } else {
        format!("{}…", chars[..72].iter().collect::<String>())
    }
}

fn commit_summary(text: &str, config: &Config) -> String {
    if config.llm {
        if let Some(s) = ollama_call(
            &config.llm_model,
            &format!("Summarize in 5-7 words, lowercase, no punctuation, output only the summary: {}", text),
        ) {
            return s;
        }
    }
    preview(text)
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

fn ollama_call(model: &str, prompt: &str) -> Option<String> {
    let body = serde_json::json!({
        "model": model,
        "prompt": prompt,
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
    let json: serde_json::Value = serde_json::from_str(&resp.into_string().ok()?).ok()?;
    let reply = json["response"].as_str()?.trim().to_string();
    if reply.is_empty() { None } else { Some(reply) }
}

fn llm_reply(text: &str, model: &str, trigger: &LlmTrigger, classifier_prompt: &str) -> Option<String> {
    let should_reply = match trigger {
        LlmTrigger::Off      => return None,
        LlmTrigger::Always   => true,
        LlmTrigger::Heuristic => text.contains('?'),
        LlmTrigger::Classifier => {
            let prompt = classifier_prompt.replace("{thought}", text);
            let verdict = ollama_call(model, &prompt)?;
            verdict.to_lowercase().starts_with("yes")
        }
    };
    if !should_reply { return None; }
    ollama_call(model, &format!("Answer in one concise sentence, no preamble: {}", text))
}

fn do_commit(text: &str, summary: &str, config: &Config) -> Result<(String, String), String> {
    let path = hm_path(config);
    let ts = Local::now().format("%Y-%m-%d %H:%M").to_string();
    let entry = format_entry(&ts, text);
    let existing = fs::read_to_string(&path).unwrap_or_default();
    fs::write(&path, format!("{}{}", entry, existing))
        .map_err(|e| format!("Failed to write {}: {}", path.display(), e))?;
    git_silent(&config.repo, &["add", &config.file])?;
    git_silent(&config.repo, &["commit", "-m", &format!("capture: {}", summary)])?;
    let hash = Command::new("git")
        .current_dir(&config.repo)
        .args(["rev-parse", "--short", "HEAD"])
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|| "???????".to_string());
    Ok((hash, ts))
}

pub(crate) fn do_capture(text: &str, config: &Config) -> Result<(String, String, Option<String>), String> {
    let summary = commit_summary(text, config);
    let (hash, ts) = do_commit(text, &summary, config)?;
    let reply = if config.llm { llm_reply(text, &config.llm_model, &config.llm_trigger, &config.llm_classifier_prompt) } else { None };
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

    let mut entries = parse_entries(&content);

    if idx >= entries.len() {
        return Err("Entry not found in file.".to_string());
    }

    let msg = format!("delete: {}", preview(&entries[idx].1));
    entries.remove(idx);

    let new_content: String = entries.iter().map(|(ts, text)| format_entry(ts, text)).collect();
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

fn loading_animation<T, F: FnOnce() -> T>(f: F) -> T {
    const WIDTH: usize = 88;
    const BARS: &[char] = &[' ', '▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];

    eprintln!();
    let _ = std::io::stderr().flush();

    let done = Arc::new(AtomicBool::new(false));
    let done_clone = done.clone();

    let handle = std::thread::spawn(move || {
        let mut phase = 0.0f64;
        loop {
            let mut line = String::with_capacity(WIDTH);
            for x in 0..WIDTH {
                let v = ((x as f64 * 0.22 + phase).sin()
                       + (x as f64 * 0.13 + phase * 1.4).sin()) / 2.0;
                let v = (v + 1.0) / 2.0;
                let idx = ((v * (BARS.len() - 1) as f64).round() as usize).min(BARS.len() - 1);
                line.push(BARS[idx]);
            }
            eprint!("\x1b[1A\r{}\n", line);
            let _ = std::io::stderr().flush();

            if done_clone.load(Ordering::Relaxed) { break; }
            phase += 0.18;
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
    });

    let result = f();
    done.store(true, Ordering::Relaxed);
    let _ = handle.join();

    eprint!("\x1b[1A\r{}\x1b[1A\r", " ".repeat(WIDTH));
    let _ = std::io::stderr().flush();

    result
}

fn cmd_capture(parts: &[String]) -> Result<(), String> {
    if parts.is_empty() {
        return Err("Provide a thought to capture.".to_string());
    }
    let text = parts.join(" ");
    let text = text.trim();
    let config = load_config()?;

    let (hash, ts, reply) = if config.llm {
        let text_owned = text.to_string();
        loading_animation(move || -> Result<(String, String, Option<String>), String> {
            let config = load_config()?;
            let summary = commit_summary(&text_owned, &config);
            let (hash, ts) = do_commit(&text_owned, &summary, &config)?;
            let reply = llm_reply(&text_owned, &config.llm_model, &config.llm_trigger, &config.llm_classifier_prompt);
            Ok((hash, ts, reply))
        })?
    } else {
        let summary = commit_summary(text, &config);
        let (hash, ts) = do_commit(text, &summary, &config)?;
        (hash, ts, None)
    };

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

    let hashes = capture_hashes(&config);
    let entries = parse_entries(&content);

    if entries.is_empty() {
        println!("No entries yet.");
        return Ok(());
    }

    for (i, (ts, text)) in entries.iter().take(count).enumerate() {
        let hash = hashes.get(i).map(|s| s.as_str()).unwrap_or("???????");
        println!("{}  {}  {}", hash, ts, text);
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
    let entries = parse_entries(&content);

    let matches: Vec<(usize, &(String, String))> = entries
        .iter()
        .enumerate()
        .filter(|(_, (ts, text))| {
            ts.to_lowercase().contains(&query_lower) || text.to_lowercase().contains(&query_lower)
        })
        .collect();

    if matches.is_empty() {
        println!("No matches.");
        return Ok(());
    }

    for (i, (ts, text)) in matches {
        let hash = hashes.get(i).map(|s| s.as_str()).unwrap_or("???????");
        println!("{}  {}  {}", hash, ts, text);
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

const CONFIG_KEYS: &[(&str, &str)] = &[
    ("repo",                  "path"),
    ("file",                  "filename"),
    ("remote",                "url"),
    ("llm",                   "true | false"),
    ("llm_model",             "any Ollama model name"),
    ("llm_trigger",           "heuristic | classifier | always | off"),
    ("llm_classifier_prompt", "string — use {thought} as placeholder"),
    ("theme",                 "laptop | eink"),
];

fn cmd_config_ls() -> Result<(), String> {
    let config = load_config()?;
    let trigger = match config.llm_trigger {
        LlmTrigger::Off        => "off",
        LlmTrigger::Heuristic  => "heuristic",
        LlmTrigger::Classifier => "classifier",
        LlmTrigger::Always     => "always",
    };
    let theme = match config.theme {
        Theme::Laptop => "laptop",
        Theme::Eink   => "eink",
    };
    let values: &[(&str, String)] = &[
        ("repo",                  config.repo.display().to_string()),
        ("file",                  config.file.clone()),
        ("remote",                config.remote.clone()),
        ("llm",                   config.llm.to_string()),
        ("llm_model",             config.llm_model.clone()),
        ("llm_trigger",           trigger.to_string()),
        ("llm_classifier_prompt", config.llm_classifier_prompt.clone()),
        ("theme",                 theme.to_string()),
    ];
    let key_width = values.iter().map(|(k, _)| k.len()).max().unwrap_or(0);
    let val_width = values.iter().map(|(_, v)| v.len().min(40)).max().unwrap_or(0);
    for (key, val) in values {
        let hint = CONFIG_KEYS.iter().find(|(k, _)| *k == *key).map(|(_, h)| *h).unwrap_or("");
        let display_val = if val.len() > 40 { format!("{}…", &val[..39]) } else { val.clone() };
        println!("{:<kw$}  =  {:<vw$}  [{}]", key, display_val, hint, kw = key_width, vw = val_width);
    }
    Ok(())
}

fn cmd_config_set(key: &str, value: &str) -> Result<(), String> {
    let valid_keys: Vec<&str> = CONFIG_KEYS.iter().map(|(k, _)| *k).collect();
    if !valid_keys.contains(&key) {
        return Err(format!(
            "Unknown config key '{}'. Valid keys: {}",
            key, valid_keys.join(", ")
        ));
    }
    let path = config_path();
    let content = fs::read_to_string(&path).map_err(|_| {
        format!("Config not found at {}. Run `hm init --repo <url>` first.", path.display())
    })?;
    let new_line = format!("{} = \"{}\"", key, value);
    let mut found = false;
    let mut new_lines: Vec<String> = content.lines().map(|line| {
        if let Some((k, _)) = line.trim().split_once('=') {
            if k.trim() == key {
                found = true;
                return new_line.clone();
            }
        }
        line.to_string()
    }).collect();
    if !found { new_lines.push(new_line); }
    fs::write(&path, new_lines.join("\n") + "\n")
        .map_err(|e| format!("Failed to write config: {}", e))?;
    println!("{} = \"{}\"", key, value);
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

fn cmd_pull() -> Result<(), String> {
    let config = load_config()?;
    git_passthrough(&config.repo, &["pull", "--rebase"])
}

// --- blog helpers ---

fn slugify(title: &str) -> String {
    title.to_lowercase()
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { '-' })
        .collect::<String>()
        .split('-')
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join("-")
}

fn age_public_key(key_path: &Path) -> Result<String, String> {
    let content = fs::read_to_string(key_path)
        .map_err(|_| format!("age key not found at {}", key_path.display()))?;
    content.lines()
        .find(|l| l.starts_with("# public key: "))
        .map(|l| l["# public key: ".len()..].to_string())
        .ok_or_else(|| "Could not parse public key from age key file".to_string())
}

fn age_encrypt(input: &Path, output: &Path, pubkey: &str) -> Result<(), String> {
    let status = Command::new("age")
        .args(["-r", pubkey, "-o"])
        .arg(output)
        .arg(input)
        .status()
        .map_err(|e| format!("Failed to run age: {}", e))?;
    if status.success() { Ok(()) } else { Err("age encryption failed".to_string()) }
}

fn age_decrypt(input: &Path, output: &Path, key_path: &Path) -> Result<(), String> {
    let status = Command::new("age")
        .args(["-d", "-i"])
        .arg(key_path)
        .args(["-o"])
        .arg(output)
        .arg(input)
        .status()
        .map_err(|e| format!("Failed to run age: {}", e))?;
    if status.success() { Ok(()) } else { Err("age decryption failed".to_string()) }
}

fn open_editor(path: &Path) -> Result<(), String> {
    let editor = std::env::var("EDITOR").unwrap_or_else(|_| "vim".to_string());
    Command::new(&editor)
        .arg(path)
        .status()
        .map_err(|e| format!("Failed to launch {}: {}", editor, e))?;
    Ok(())
}

fn blog_dir(config: &Config) -> Result<PathBuf, String> {
    config.blog_repo.as_ref()
        .map(|p| p.join("blog"))
        .ok_or_else(|| "blog_repo not set in config; add it with `hm config set blog_repo <path>`".to_string())
}

fn require_age_key(config: &Config) -> Result<PathBuf, String> {
    config.age_key.clone()
        .ok_or_else(|| "age_key not set in config; add it with `hm config set age_key ~/.config/hm/age.key`".to_string())
}

fn read_manifest(blog: &Path) -> Result<serde_json::Value, String> {
    let path = blog.join("manifest.json");
    let content = fs::read_to_string(&path)
        .map_err(|_| "manifest.json not found in blog repo".to_string())?;
    serde_json::from_str(&content).map_err(|e| format!("Failed to parse manifest: {}", e))
}

fn write_manifest(blog: &Path, manifest: &serde_json::Value) -> Result<(), String> {
    let content = serde_json::to_string_pretty(manifest)
        .map_err(|e| format!("Failed to serialize manifest: {}", e))?;
    fs::write(blog.join("manifest.json"), content + "\n")
        .map_err(|e| format!("Failed to write manifest: {}", e))
}

fn cmd_write(title_parts: &[String]) -> Result<(), String> {
    if title_parts.is_empty() {
        return Err("Provide a title.".to_string());
    }
    let title = title_parts.join(" ");
    let slug = slugify(&title);
    let config = load_config()?;
    let blog = blog_dir(&config)?;
    let key_path = require_age_key(&config)?;
    let pubkey = age_public_key(&key_path)?;

    let drafts_dir = blog.join("drafts");
    fs::create_dir_all(&drafts_dir)
        .map_err(|e| format!("Failed to create drafts dir: {}", e))?;

    let encrypted_path = drafts_dir.join(format!("{}.json.age", slug));
    if encrypted_path.exists() {
        return Err(format!("Draft '{}' already exists.", slug));
    }

    let date = Local::now().format("%Y-%m-%d").to_string();
    let skeleton = serde_json::json!({
        "slug": slug,
        "title": title,
        "date": date,
        "tags": [],
        "chunks": [{ "type": "text", "content": [""] }]
    });
    let tmp = std::env::temp_dir().join(format!("hm-{}.json", slug));
    fs::write(&tmp, serde_json::to_string_pretty(&skeleton).unwrap())
        .map_err(|e| format!("Failed to write temp file: {}", e))?;

    open_editor(&tmp)?;

    age_encrypt(&tmp, &encrypted_path, &pubkey)?;
    let _ = fs::remove_file(&tmp);

    // update manifest
    let mut manifest = read_manifest(&blog)?;
    let next_id = manifest.as_array()
        .map(|a| a.len() + 1)
        .unwrap_or(1)
        .to_string();
    let entry = serde_json::json!({
        "id": next_id,
        "slug": slug,
        "name": title,
        "timestamp": format!("{}T00:00:00Z", date),
        "tags": [],
        "type": "blog",
        "published": false
    });
    manifest.as_array_mut()
        .ok_or("manifest is not an array")?
        .push(entry);
    write_manifest(&blog, &manifest)?;

    let blog_repo = config.blog_repo.as_ref().unwrap();
    git_silent(blog_repo, &["add", "blog/"])?;
    git_silent(blog_repo, &["commit", "-m", &format!("draft: {}", title)])?;

    println!("draft: {}", slug);
    Ok(())
}

fn cmd_draft_ls() -> Result<(), String> {
    let config = load_config()?;
    let blog = blog_dir(&config)?;
    let manifest = read_manifest(&blog)?;
    let empty = vec![];
    let drafts: Vec<&serde_json::Value> = manifest.as_array()
        .unwrap_or(&empty)
        .iter()
        .filter(|e| e["published"] != true)
        .collect();
    if drafts.is_empty() {
        println!("No drafts.");
        return Ok(());
    }
    for d in drafts {
        let slug = d["slug"].as_str().unwrap_or("?");
        let name = d["name"].as_str().unwrap_or("?");
        let ts = d["timestamp"].as_str().unwrap_or("?").get(..10).unwrap_or("?");
        println!("{}  {}  {}", ts, slug, name);
    }
    Ok(())
}

fn cmd_publish(slug: &str) -> Result<(), String> {
    let config = load_config()?;
    let blog = blog_dir(&config)?;
    let key_path = require_age_key(&config)?;

    let encrypted_path = blog.join("drafts").join(format!("{}.json.age", slug));
    if !encrypted_path.exists() {
        return Err(format!("Draft '{}' not found.", slug));
    }

    let published_path = blog.join(format!("{}.json", slug));
    age_decrypt(&encrypted_path, &published_path, &key_path)?;
    fs::remove_file(&encrypted_path)
        .map_err(|e| format!("Failed to remove draft: {}", e))?;

    let mut manifest = read_manifest(&blog)?;
    let title = manifest.as_array_mut()
        .ok_or("manifest is not an array")?
        .iter_mut()
        .find(|e| e["slug"].as_str() == Some(slug))
        .map(|e| {
            e["published"] = serde_json::Value::Bool(true);
            e["name"].as_str().unwrap_or(slug).to_string()
        })
        .ok_or_else(|| format!("'{}' not found in manifest.", slug))?;
    write_manifest(&blog, &manifest)?;

    let blog_repo = config.blog_repo.as_ref().unwrap();
    git_silent(blog_repo, &["add", "blog/"])?;
    git_silent(blog_repo, &["commit", "-m", &format!("publish: {}", title)])?;

    println!("publish: {}", slug);
    Ok(())
}

fn cmd_blog_demote(slug: &str) -> Result<(), String> {
    let config = load_config()?;
    let blog = blog_dir(&config)?;
    let key_path = require_age_key(&config)?;
    let pubkey = age_public_key(&key_path)?;

    let published_path = blog.join(format!("{}.json", slug));
    if !published_path.exists() {
        return Err(format!("Published post '{}' not found.", slug));
    }

    let drafts_dir = blog.join("drafts");
    fs::create_dir_all(&drafts_dir)
        .map_err(|e| format!("Failed to create drafts dir: {}", e))?;
    let encrypted_path = drafts_dir.join(format!("{}.json.age", slug));

    age_encrypt(&published_path, &encrypted_path, &pubkey)?;
    fs::remove_file(&published_path)
        .map_err(|e| format!("Failed to remove published post: {}", e))?;

    let mut manifest = read_manifest(&blog)?;
    let title = manifest.as_array_mut()
        .ok_or("manifest is not an array")?
        .iter_mut()
        .find(|e| e["slug"].as_str() == Some(slug))
        .map(|e| {
            e["published"] = serde_json::Value::Bool(false);
            e["name"].as_str().unwrap_or(slug).to_string()
        })
        .ok_or_else(|| format!("'{}' not found in manifest.", slug))?;
    write_manifest(&blog, &manifest)?;

    let blog_repo = config.blog_repo.as_ref().unwrap();
    git_silent(blog_repo, &["add", "blog/"])?;
    git_silent(blog_repo, &["commit", "-m", &format!("draft: {}", title)])?;

    println!("demoted: {}", slug);
    Ok(())
}

fn cmd_blog_ls() -> Result<(), String> {
    let config = load_config()?;
    let blog = blog_dir(&config)?;
    let manifest = read_manifest(&blog)?;
    let empty = vec![];
    let posts: Vec<&serde_json::Value> = manifest.as_array()
        .unwrap_or(&empty)
        .iter()
        .filter(|e| e["published"] == true)
        .collect();
    if posts.is_empty() {
        println!("No published posts.");
        return Ok(());
    }
    for p in posts {
        let slug = p["slug"].as_str().unwrap_or("?");
        let name = p["name"].as_str().unwrap_or("?");
        let ts = p["timestamp"].as_str().unwrap_or("?").get(..10).unwrap_or("?");
        println!("{}  {}  {}", ts, slug, name);
    }
    Ok(())
}

fn cmd_blog_push() -> Result<(), String> {
    let config = load_config()?;
    let blog_repo = config.blog_repo.as_ref()
        .ok_or("blog_repo not set in config")?;
    if git_silent(blog_repo, &["push"]).is_err() {
        git_silent(blog_repo, &["push", "-u", "origin", "HEAD"])?;
    }
    let remote = Command::new("git")
        .current_dir(blog_repo)
        .args(["remote", "get-url", "origin"])
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|| "remote".to_string());
    println!("Pushed → {}", remote);
    Ok(())
}

fn cmd_log(count: usize) -> Result<(), String> {
    let config = load_config()?;
    let out = Command::new("git")
        .current_dir(&config.repo)
        .args([
            "log",
            &format!("-{}", count),
            "--format=%h %ad %s",
            "--date=format:%Y-%m-%d %H:%M",
            "--",
            &config.file,
        ])
        .output()
        .map_err(|e| format!("Failed to run git log: {}", e))?;
    let text = String::from_utf8_lossy(&out.stdout);
    if text.trim().is_empty() {
        println!("No history yet.");
    } else {
        print!("{}", text);
    }
    Ok(())
}

fn main() {
    if std::env::args().len() == 1 {
        if let Err(e) = tui::run() {
            eprintln!("error: {}", e);
            std::process::exit(1);
        }
        return;
    }

    let cli = Cli::parse();
    let result = match cli.command {
        Commands::Ls { count } => cmd_ls(count),
        Commands::Delete { hash } => cmd_delete(&hash),
        Commands::Init { repo } => cmd_init(&repo),
        Commands::Push => cmd_push(),
        Commands::Search { query } => cmd_search(&query),
        Commands::View { path } => cmd_view(&path),
        Commands::Log { count } => cmd_log(count),
        Commands::Pull => cmd_pull(),
        Commands::Write { title } => cmd_write(&title),
        Commands::Draft { cmd } => match cmd {
            DraftCmd::Ls => cmd_draft_ls(),
        },
        Commands::Publish { slug } => cmd_publish(&slug),
        Commands::Blog { cmd } => match cmd {
            BlogCmd::Ls => cmd_blog_ls(),
            BlogCmd::Push => cmd_blog_push(),
            BlogCmd::Demote { slug } => cmd_blog_demote(&slug),
        },
        Commands::Config { cmd } => match cmd {
            ConfigCmd::Ls => cmd_config_ls(),
            ConfigCmd::Set { key, value } => cmd_config_set(&key, &value),
        },
        Commands::Capture(parts) => cmd_capture(&parts),
    };

    if let Err(e) = result {
        eprintln!("error: {}", e);
        std::process::exit(1);
    }
}
