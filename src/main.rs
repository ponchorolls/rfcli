use clap::{Parser, Subcommand};
use skim::prelude::*;
use regex::Regex;
use ollama_rs::generation::completion::request::GenerationRequest;
use ollama_rs::Ollama;
use colored::Colorize; 
use std::process::{Command, Stdio};
use std::io::Write;
use std::io::Cursor;
use std::fs;
use std::path::PathBuf;



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
    },
    /// Get a summarized TLDR of an RFC
    Tldr { number: u32 },
}

#[tokio::main]
async fn main() {
    let cli = Cli::parse();

    match &cli.command {
    Commands::Read { refresh } => {
    let mut first_run = *refresh; // Only refresh on the first loop if requested
    
    loop {
        print!("\x1B[2J\x1B[1;1H");
        // 1. Open the fuzzy finder
        if let Some(rfc_num) = fuzzy_select_rfc(first_run) {
            first_run = false; // Don't force re-downloading on subsequent loops
            
            println!("Fetching RFC {}...", rfc_num);
            
            match fetch_rfc(rfc_num).await {
                Ok(content) => {
                    let cleaned = clean_rfc_text(&content);
                    // 2. Open the pager. When you hit 'q', it returns here!
                    view_in_pager(&cleaned);
                }
                Err(e) => {
                    eprintln!("Error: {}", e);
                    // Give the user a moment to see the error before looping back
                    std::thread::sleep(std::time::Duration::from_secs(2));
                }
            }
        } else {
            // 3. If the user hits Esc or Ctrl+C in skim, it returns None.
            // This is our exit condition for the loop.
            println!("Exiting rfcli...");
            break;
        }
    }
    }
        Commands::Tldr { number } => {
            match fetch_rfc(*number).await {
                Ok(content) => generate_tldr(&content).await,
                Err(e) => eprintln!("Failed to fetch RFC: {}", e),
            }
        }
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

fn fuzzy_select_rfc(force_refresh: bool) -> Option<u32> {
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

    let options = SkimOptionsBuilder::default()
        .height(Some("50%"))
        .multi(false)
        .bind(vec!["esc:abort", "ctrl-c:abort"]) // Force Bind
        .build()
        .unwrap();

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

async fn generate_tldr(text: &str) {
    let ollama = Ollama::default();
    let cleaned_text = clean_rfc_text(text);
    
    let context_window: String = cleaned_text.lines()
        .take(300) 
        .collect::<Vec<&str>>()
        .join("\n");

    let prompt = format!(
        "You are an expert networking engineer. Summarize this RFC technical document. \
         Focus on: \n\
         1. What problem does it solve?\n\
         2. Key protocol mechanisms.\n\
         3. Target use cases.\n\n\
         Keep it concise and use bullet points.\n\n\
         RFC Content:\n{}", 
        context_window
    );

    println!("{}", "--- Generating Summary (Local LLM via Ollama) ---".bold().cyan());

    let res = ollama
        .generate(GenerationRequest::new("llama3".to_string(), prompt))
        .await;

    match res {
        Ok(response) => {
            println!("\n{}", "Summary:".bold().green());
            println!("{}", response.response);
        }
        Err(e) => {
            eprintln!("\n{}: {}", "Error calling Ollama".red().bold(), e);
        }
    }
}

fn view_in_pager(content: &str) {
    // We pass -K directly to the pager command
    let (cmd, args) = if Command::new("bat").arg("--version").stdout(Stdio::null()).status().is_ok() {
        ("bat", vec!["--paging=always", "--pager=less -K"])
    } else {
        ("less", vec!["-K"])
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


