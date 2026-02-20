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
use indicatif::{ProgressBar, ProgressStyle};


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
        number: u32,
        #[arg(short, long, default_value = "llama3")]
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
        
        Commands::Tldr { number, model } => {
            match fetch_rfc(*number).await {
                Ok(content) => generate_tldr(*number, &content, model).await,
                Err(e) => eprintln!("Error: {}", e),
            }
        }
    }
}

async fn generate_tldr(number: u32, text: &str, model: &str) {
    let ollama = Ollama::default();
    let cleaned_text = clean_rfc_text(text);
    
    // INTEL OPTIMIZATION: Take only the first 80 lines (Abstract)
    let abstract_text: String = cleaned_text.lines().take(80).collect::<Vec<_>>().join("\n");

    // Search for Security, but only grab 30 lines
    let security_re = Regex::new(r"(?i)Security Considerations").unwrap();
    let security_text = if let Some(m) = security_re.find(&cleaned_text) {
        cleaned_text[m.start()..].lines().take(30).collect::<Vec<_>>().join("\n")
    } else {
        "N/A".to_string()
    };

    let prompt = format!(
        "Briefly summarize RFC {}. One sentence pitch, 3 technical bullets, one security risk. \
        \n\nABS: {}\n\nSEC: {}", 
        number, abstract_text, security_text
    );

    let pb = ProgressBar::new_spinner();
    // Custom message to remind you it's a CPU-heavy task
    pb.set_message("CPU is crunching numbers (this may take 10-20s)...");
    pb.enable_steady_tick(std::time::Duration::from_millis(120));

    let res = ollama
        .generate(GenerationRequest::new(model.to_string(), prompt))
        .await;

    pb.finish_and_clear();
    // let pb = ProgressBar::new_spinner();
    // pb.set_style(ProgressStyle::default_spinner()
    //     .template("{spinner:.green} {msg}")
    //     .unwrap());
    // pb.set_message("Architecting summary...");
    // pb.enable_steady_tick(std::time::Duration::from_millis(120));

    // // The actual call
    // let res = ollama.generate(GenerationRequest::new(model.to_string(), prompt)).await;

    // pb.finish_and_clear(); 

    match res {
        Ok(response) => {
            println!("{}", format!("--- Analyzing RFC {} via {} ---", number, model).bold().cyan());
            println!("\n{}", response.response);
        }
        Err(e) => eprintln!("Error calling Ollama: {}", e),
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
    // let options = SkimOptionsBuilder::default()
    //     .height(Some("50%"))
    //     .multi(false)
    //     .bind(vec!["esc:abort", "ctrl-c:abort"]) // Force Bind
    //     .build()
    //     .unwrap();

    // let output = Skim::run_with(&options, Some(items));

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

// fn view_in_pager(content: &str) {
//     // -l man: Use the manpage syntax highlighter
//     // -p: Plain mode (no grid/header)
//     // -K: (Passed to less) Quit on ESC/Ctrl+C
//     let mut child = Command::new("bat")
//         .args(["-l", "man", "-p", "--pager", "less -FK"])
//         .stdin(Stdio::piped())
//         .spawn()
//         .unwrap_or_else(|_| {
//             Command::new("less")
//                 .arg("-FK")
//                 .stdin(Stdio::piped())
//                 .spawn()
//                 .expect("Failed to spawn pager")
//         });

//     if let Some(mut stdin) = child.stdin.take() {
//         let _ = stdin.write_all(content.as_bytes());
//     }

//     let _ = child.wait();
// }

// fn view_in_pager(content: &str) {
//     // We pass -K directly to the pager command
//     let (cmd, args) = if Command::new("bat").arg("--version").stdout(Stdio::null()).status().is_ok() {
//         ("bat", vec!["--paging=always", "--pager=less -K"])
//     } else {
//         ("less", vec!["-K"])
//     };

//     let mut child = Command::new(cmd)
//         .args(args)
//         .stdin(Stdio::piped())
//         .spawn()
//         .expect("Failed to spawn pager");

//     if let Some(mut stdin) = child.stdin.take() {
//         let _ = stdin.write_all(content.as_bytes());
//     }

//     let _ = child.wait();
// }
