use clap::{Parser, Subcommand};
use skim::prelude::*;
use std::io::Cursor;

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
