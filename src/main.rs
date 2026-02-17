use clap::{Parser, Subcommand};
use skim::prelude::*;
use std::io::Cursor;
use regex::Regex;
use ollama_rs::generation::completion::request::GenerationRequest;
use ollama_rs::Ollama;
use colored::Colorize; // Necessary for .bold().cyan()

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
    Read,
    /// Get a summarized TLDR of an RFC
    Tldr { number: u32 },
}

#[tokio::main]
async fn main() {
    let cli = Cli::parse();

    match &cli.command {
        Commands::Read => {
            if let Some(rfc_num) = fuzzy_select_rfc() {
                println!("Fetching RFC {}...", rfc_num);
                // We make this async now
                match fetch_rfc(rfc_num).await {
                    Ok(content) => println!("{}", content),
                    Err(e) => eprintln!("Failed to fetch RFC: {}", e),
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

async fn fetch_rfc(number: u32) -> Result<String, reqwest::Error> {
    let url = format!("https://www.rfc-editor.org/rfc/rfc{}.txt", number);
    // Use the async client
    reqwest::get(url).await?.text().await
}

fn clean_rfc_text(raw_text: &str) -> String {
    let no_feeds = raw_text.replace('\x0C', "");
    let header_footer_re = Regex::new(r"(?m)^.*\[Page \d+\].*$|^RFC \d+.*$").unwrap();
    let cleaned = header_footer_re.replace_all(&no_feeds, "");
    let multi_space_re = Regex::new(r"\n{3,}").unwrap();
    multi_space_re.replace_all(&cleaned, "\n\n").to_string()
}

fn fuzzy_select_rfc() -> Option<u32> {
    let options = "791: Internet Protocol\n2616: HTTP/1.1\n1035: DNS";
    let item_reader = SkimItemReader::default();
    let items = item_reader.of_bufread(Cursor::new(options));

    let selected_items = Skim::run_with(&SkimOptionsBuilder::default().build().unwrap(), Some(items))
        .map(|out| out.selected_items)
        .unwrap_or_else(|| Vec::new());

    selected_items.first().and_then(|item| {
        item.output().split(':').next()?.parse::<u32>().ok()
    })
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