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
    /// Open the interactive TUI
    Tui,
    /// Show commit history of the notes file
    Log {
        #[arg(short = 'n', long, default_value = "20")]
        count: usize,
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

fn do_commit(text: &str, config: &Config) -> Result<(String, String), String> {
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
    Ok((hash, ts))
}

pub(crate) fn do_capture(text: &str, config: &Config) -> Result<(String, String, Option<String>), String> {
    let (hash, ts) = do_commit(text, config)?;
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

fn load_word_pool(path: &Path) -> Vec<String> {
    const STOP: &[&str] = &[
        "the","and","for","are","but","not","you","all","can","was","one","our","out",
        "get","has","him","his","how","its","may","new","now","see","two","way","who",
        "did","she","too","use","that","with","have","this","from","they","know","want",
        "been","good","much","some","time","very","when","come","here","just","like",
        "long","make","many","more","only","over","such","take","than","them","well",
        "were","what","your","into","also","back","even","most","will","about","would",
        "there","their","could","other","these","those","after","where","which","while",
        "being","doing","each","then","should","through","because","before",
    ];
    let content = fs::read_to_string(path).unwrap_or_default();
    let mut counts: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
    for raw in content.split_whitespace() {
        for part in raw.split(|c: char| !c.is_alphabetic()) {
            let w = part.to_lowercase();
            if w.len() >= 4 && !STOP.contains(&w.as_str()) {
                *counts.entry(w).or_insert(0) += 1;
            }
        }
    }
    let mut pool = Vec::new();
    for (word, count) in counts {
        for _ in 0..count.min(4) {
            pool.push(word.clone());
        }
    }
    pool
}

fn word_cloud_loader<F: FnOnce() -> Option<String>>(pool: Vec<String>, f: F) -> Option<String> {
    if pool.is_empty() {
        return f();
    }

    const ROWS: usize = 8;
    const WIDTH: usize = 72;
    const SLOTS: &[(usize, usize)] = &[
        (0, 4),  (0, 40),
        (1, 16), (1, 50),
        (2, 6),  (2, 44),
        (3, 0),  (3, 28), (3, 56),
        (4, 10), (4, 46),
        (5, 4),  (5, 36),
        (6, 18), (6, 52),
        (7, 8),  (7, 42),
    ];

    for _ in 0..ROWS { eprintln!(); }
    let _ = std::io::stderr().flush();

    let done = Arc::new(AtomicBool::new(false));
    let done_clone = done.clone();

    let handle = std::thread::spawn(move || {
        let seed = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.subsec_nanos() as u64)
            .unwrap_or(42);
        let mut state = seed;
        let mut rng = move |n: usize| -> usize {
            state = state.wrapping_mul(6364136223846793005)
                         .wrapping_add(1442695040888963407);
            (state >> 33) as usize % n
        };

        struct Slot { row: usize, col: usize, word: String, life: usize, visible: bool }
        let mut slots: Vec<Slot> = SLOTS.iter().map(|&(row, col)| {
            Slot { row, col, word: String::new(), life: 0, visible: false }
        }).collect();
        let mut reveal_idx = 0usize;

        loop {
            if reveal_idx < slots.len() {
                let idx = rng(pool.len());
                slots[reveal_idx].word = pool[idx].clone();
                slots[reveal_idx].life = rng(4) + 6;
                slots[reveal_idx].visible = true;
                reveal_idx += 1;
            }

            let mut grid: Vec<Vec<char>> = vec![vec![' '; WIDTH]; ROWS];
            for slot in slots.iter().filter(|s| s.visible) {
                for (i, ch) in slot.word.chars().enumerate() {
                    let c = slot.col + i;
                    if c < WIDTH { grid[slot.row][c] = ch; }
                }
            }

            eprint!("\x1b[{}A\r", ROWS);
            for row in &grid {
                eprintln!("{}", row.iter().collect::<String>());
            }
            let _ = std::io::stderr().flush();

            if done_clone.load(Ordering::Relaxed) { break; }

            if reveal_idx >= slots.len() {
                for slot in &mut slots {
                    if slot.life > 0 { slot.life -= 1; }
                    if slot.life == 0 {
                        let idx = rng(pool.len());
                        slot.word = pool[idx].clone();
                        slot.life = rng(4) + 6;
                    }
                }
            }

            std::thread::sleep(std::time::Duration::from_millis(350));
        }
    });

    let result = f();
    done.store(true, Ordering::Relaxed);
    let _ = handle.join();

    eprint!("\x1b[{}A\r", ROWS);
    for _ in 0..ROWS { eprintln!("{}", " ".repeat(WIDTH)); }

    let output_lines = if result.is_some() { 2usize } else { 1 };
    let center_row = (ROWS - output_lines) / 2;
    eprint!("\x1b[{}A\r", ROWS - center_row);
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
    let (hash, ts) = do_commit(text, &config)?;
    let reply = if config.llm {
        let pool = load_word_pool(&hm_path(&config));
        let model = config.llm_model.clone();
        let trigger = config.llm_trigger.clone();
        let classifier_prompt = config.llm_classifier_prompt.clone();
        let text_owned = text.to_string();
        word_cloud_loader(pool, move || {
            llm_reply(&text_owned, &model, &trigger, &classifier_prompt)
        })
    } else {
        None
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
    let cli = Cli::parse();
    let result = match cli.command {
        Commands::Ls { count } => cmd_ls(count),
        Commands::Delete { hash } => cmd_delete(&hash),
        Commands::Init { repo } => cmd_init(&repo),
        Commands::Push => cmd_push(),
        Commands::Search { query } => cmd_search(&query),
        Commands::View { path } => cmd_view(&path),
        Commands::Tui => tui::run(),
        Commands::Log { count } => cmd_log(count),
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
