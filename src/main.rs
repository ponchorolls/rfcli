use clap::{Parser, Subcommand};
use skim::prelude::*;
use regex::Regex;
use colored::Colorize; 
use std::process::{Command, Stdio};
use std::io::Write;
use std::io::Cursor;
use std::fs;
use std::path::PathBuf;
use indicatif::{ProgressBar, ProgressStyle};
use serde_json::json;
use textwrap::{wrap, Options};


#[derive(Parser)]
#[command(name = "rfc")]
#[command(about = "A fast RFC reader with fuzzy search and TLDR", long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Search and read an RFC
    Read {
        /// Force update the local RFC index
        #[arg(short, long)]
        refresh: bool,
        #[arg(short, long)]
        query: Option<String>,
    },
    /// Get a summarized TLDR of an RFC
    Tldr { 
        number: Option<u32>,
        #[arg(short, long, default_value = "llama-3.1-8b-instant")]
        model: String
    },
}

#[tokio::main]
async fn main() {
    let cli = Cli::parse();

    match &cli.command {
        Commands::Read { refresh, query } => {
            let mut first_run = *refresh;
            let mut initial_query = query.clone();
            loop {
                // We don't want to clear the screen if we're just printing an error
                if let Some(rfc_num) = fuzzy_select_rfc(first_run, initial_query.take()) {
                    first_run = false;
                    println!("Fetching RFC {}...", rfc_num);
                    
                    match fetch_rfc(rfc_num).await {
                        Ok(content) => {
                            let cleaned = clean_rfc_text(&content);
                            view_in_pager(&cleaned);
                        }
                        Err(e) => {
                            eprintln!("Error: {}", e);
                            std::thread::sleep(std::time::Duration::from_secs(2));
                        }
                    }
                } else {
                    println!("Exiting rfcli...");
                    break;
                }
            }
        } // Closing brace for Read arm
        
        Commands::Tldr { number, model} => {
            // 1. Determine the number: use the argument if provided, otherwise search
    let target_number = match number {
        Some(n) => Some(*n),
        None => fuzzy_select_rfc(false, None), // Use our existing search!
    };

    // 2. If we have a number (either from arg or search), proceed
    if let Some(n) = target_number {
        match fetch_rfc(n).await {
            Ok(content) => generate_tldr(n, &content, model).await,
            Err(e) => eprintln!("Error fetching RFC {}: {}", n, e),
        }
    } else {
        println!("No RFC selected. Exiting...");
    }
}
}
async fn generate_tldr(number: u32, text: &str, model: &str) {
    let api_key = std::env::var("GROQ_API_KEY")
        .expect("Please set the GROQ_API_KEY environment variable");

    let cleaned_text = clean_rfc_text(text);
    let context = cleaned_text.lines().take(300).collect::<Vec<_>>().join("\n");

    let pb = ProgressBar::new_spinner();
    pb.set_style(ProgressStyle::default_spinner()
        .tick_strings(&["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"])
        .template("{spinner:.magenta} {msg}")
        .unwrap());
    pb.set_message("Querying Groq Cloud...");
    pb.enable_steady_tick(std::time::Duration::from_millis(120));

    let client = reqwest::Client::new();
    let res = client
        .post("https://api.groq.com/openai/v1/chat/completions")
        .header("Authorization", format!("Bearer {}", api_key))
        .json(&json!({
            "model": "llama-3.1-8b-instant",
            "messages": [
                {
                    "role": "system",
                    "content": "You are a Senior Systems Engineer. Summarize the RFC for a terminal UI. DO NOT use Markdown bolding (no asterisks). Use a simple 'TITLE: description' format for bullets. Keep the elevator pitch at the top."
                },
                {
                    "role": "user",
                    "content": format!("Summarize RFC {}:\n\n{}", number, context)
                }
            ]
        }))
        .send()
        .await;

    pb.finish_and_clear();

    match res {
        Ok(response) => {
            let body = response.text().await.unwrap_or_default();
            let v: serde_json::Value = serde_json::from_str(&body).unwrap_or_default();
            
            // The 'if let' block now contains all the printing logic to keep 'summary' in scope
            if let Some(summary_text) = v["choices"][0]["message"]["content"].as_str() {
                // 1. Detect terminal width (defaults to 80 if it can't detect)
                let term_width = termsize::get().map(|t| t.cols as usize).unwrap_or(80);
                // 2. Set wrapping options (leaving a little margin for our box/indent)
                let wrap_options = Options::new(term_width - 6);

                println!("\n{}", "╭──────────────────────────────────────────────────────────╮".cyan().bold());
                println!("  {} {} {}", "🚀".bold(), "RFC".bold(), number.to_string().bold().yellow());
                println!("{}", "╰──────────────────────────────────────────────────────────╯".cyan().bold());
                
                for line in summary_text.lines() {
                    let trimmed = line.trim();
                    if trimmed.is_empty() { continue; }

                    // Skip conversational filler from the AI
                    let lower = trimmed.to_lowercase();
                    if lower.starts_with("here is") || lower.contains("summary of rfc") {
                        continue;
                    }

                    // Clean and print with high contrast for the X220 screen
                    let clean_line = trimmed.replace("**", "");
                    // 3. Wrap the cleaned line
                    let wrapped_lines = wrap(&clean_line, &wrap_options);

                    for (i, wrapped) in wrapped_lines.iter().enumerate() {
                        if i == 0 && (clean_line.starts_with('*') || clean_line.starts_with('-')) {
                            // First line of a bullet point gets the bullet
                            println!("  {} {}", "•".cyan().bold(), wrapped[1..].trim().white().bold());
                        } else {
                            // Subsequent wrapped lines are indented to match
                            println!("    {}", wrapped.white().bold());
                        }
                    }
                }
            } else {
                eprintln!("{}: API response did not contain a summary.", "Error".red());
                println!("Debug: {}", body);
            }
        }
        Err(e) => eprintln!("{}: {}", "Network Error".red(), e),
    }
}

// --- Logic Functions ---

async fn fetch_rfc(number: u32) -> Result<String, Box<dyn std::error::Error>> {
    let cache_path = dirs::cache_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("rfcli")
        .join(format!("rfc{}.txt", number));

    // If it's in the cache, read it!
    if cache_path.exists() {
        return Ok(fs::read_to_string(cache_path)?);
    }

    // Otherwise, fetch and save it
    let url = format!("https://www.rfc-editor.org/rfc/rfc{}.txt", number);
    let content = reqwest::get(url).await?.text().await?;
    
    // Save for next time
    let _ = fs::write(cache_path, &content);
    
    Ok(content)
}

fn clean_rfc_text(raw_text: &str) -> String {
    let no_feeds = raw_text.replace('\x0C', "");
    let header_footer_re = Regex::new(r"(?m)^.*\[Page \d+\].*$|^RFC \d+.*$").unwrap();
    let cleaned = header_footer_re.replace_all(&no_feeds, "");
    let multi_space_re = Regex::new(r"\n{3,}").unwrap();
    multi_space_re.replace_all(&cleaned, "\n\n").to_string()
}

fn fuzzy_select_rfc(force_refresh: bool, query: Option<String>) -> Option<u32> {
    let cache_dir = dirs::cache_dir()?.join("rfcli");
    let index_path = cache_dir.join("rfc-index.txt");

    if !cache_dir.exists() {
        fs::create_dir_all(&cache_dir).ok()?;
    }

    // Download if it doesn't exist OR if user passed the -r flag
    if !index_path.exists() || force_refresh {
        println!("{}", "Updating RFC index from IETF...".yellow());
        let response = reqwest::blocking::get("https://www.rfc-editor.org/rfc/rfc-index.txt").ok()?;
        let content = response.text().ok()?;
        fs::write(&index_path, content).ok()?;
        println!("{}", "Index updated successfully.".green());
    }

    let index_data = fs::read_to_string(index_path).ok()?;
    
    let filtered_index: String = index_data.lines()
        .filter(|line| line.trim().chars().next().map(|c| c.is_ascii_digit()).unwrap_or(false))
        .collect::<Vec<_>>()
        .join("\n");

    let item_reader = SkimItemReader::default();
    let items = item_reader.of_bufread(Cursor::new(filtered_index));

    let mut options_builder = SkimOptionsBuilder::default();
    options_builder
        .height(Some("50%"))
        .multi(false)
        .bind(vec!["esc:abort", "ctrl-c:abort"]);

    // If a query was provided, set it as the initial search text
    if let Some(ref q) = query {
        options_builder.query(Some(q));
    }

    let options = options_builder.build().unwrap();
    let output = Skim::run_with(&options, Some(items));

    // Check if the user aborted (pressed ESC)
    if let Some(out) = output {
        if out.final_event == Event::EvActAbort {
            return None; // This will trigger the 'break' in your loop
        }
        
        out.selected_items.first().and_then(|item| {
            item.output().split_whitespace().next()?.parse::<u32>().ok()
        })

        
    } else {
        None
    }
}

// Ensure there is only ONE argument here: content
fn view_in_pager(content: &str) {
    let (cmd, args) = if Command::new("bat").arg("--version").stdout(Stdio::null()).status().is_ok() {
        // We use the 'man' language and 'plain' flags for those nice colors
        ("bat", vec!["-l", "man", "-p", "--pager", "less -FK"])
    } else {
        ("less", vec!["-FK"])
    };

    let mut child = Command::new(cmd)
        .args(args)
        .stdin(Stdio::piped())
        .spawn()
        .expect("Failed to spawn pager");

    if let Some(mut stdin) = child.stdin.take() {
        let _ = stdin.write_all(content.as_bytes());
    }

    let _ = child.wait();
}
}
