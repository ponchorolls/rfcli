use clap::{Parser, Subcommand};
use skim::prelude::*;
use std::io::Cursor;
use regex::Regex;
use ollama_rs::generation::completion::request::GenerationRequest;
use ollama_rs::Ollama;
use std::io::BufRead;

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
                let content = fetch_rfc(rfc_num);
                println!("{}", content);
            }
        }
        Commands::Tldr { number } => {
            let content = fetch_rfc(*number);
            generate_tldr(&content).await;
        }
    }
}


fn clean_rfc_text(raw_text: &str) -> String {
    // 1. Remove Form Feed characters
    let no_feeds = raw_text.replace('\x0C', "");

    // 2. Regex to catch typical RFC headers/footers
    // Example: "RFC 2616              HTTP/1.1               June 1999"
    // or "[Page 12]"
    let header_footer_re = Regex::new(r"(?m)^.*\[Page \d+\].*$|^RFC \d+.*$").unwrap();
    let cleaned = header_footer_re.replace_all(&no_feeds, "");

    // 3. Optional: Collapse multiple newlines into double newlines
    let multi_space_re = Regex::new(r"\n{3,}").unwrap();
    multi_space_re.replace_all(&cleaned, "\n\n").to_string()
}

fn fuzzy_select_rfc() -> Option<u32> {
    // In a real app, you'd load a list of titles from a local cache/index
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

fn fetch_rfc(number: u32) -> String {
    let url = format!("https://www.rfc-editor.org/rfc/rfc{}.txt", number);
    reqwest::blocking::get(url).expect("Failed to fetch").text().expect("Failed to read body")
}

async fn generate_tldr(text: &str) {
    println!("--- Generating TLDR via Ollama ---");
    // Implementation for Ollama-rs goes here
    // Tip: Send only the first 200 lines to avoid token limits
}

async fn generate_tldr(text: &str) {
    let ollama = Ollama::default();
    
    // Clean the text first!
    let cleaned_text = clean_rfc_text(text);
    
    // Grab the first ~5000 characters (plenty for Abstract + Intro)
    let context_window = cleaned_text.chars().take(5000).collect::<String>();

    let prompt = format!(
        "You are an expert networking engineer. Summarize this RFC. \
         Be concise. Use bullet points for key features. \n\n\
         Content:\n{}", 
        context_window
    );

    // ... (rest of the Ollama call from the previous step)
}


// ... (previous imports and Cli/Commands structs)

async fn generate_tldr(text: &str) {
    let ollama = Ollama::default();
    
    // 1. Extract the "Meat": Usually the first 300 lines cover Abstract/Intro
    let relevant_text: String = text.lines()
        .take(300) 
        .collect::<Vec<&str>>()
        .join("\n");

    let prompt = format!(
        "Summarize this RFC technical document. Focus on: \n\
         1. What problem does it solve?\n\
         2. Key protocol mechanisms.\n\
         3. Target use cases.\n\n\
         RFC Content:\n{}", 
        relevant_text
    );

    println!("{}", "--- Generating Summary (Local LLM) ---".bold().cyan());

    let res = ollama
        .generate(GenerationRequest::new("llama3".to_string(), prompt))
        .await;

    match res {
        Ok(response) => println!("\n{}", response.response),
        Err(e) => eprintln!("Error calling Ollama: {}. Is the service running?", e),
    }
}
