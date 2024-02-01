use clap::Parser;
use dockim::cli::Args;

fn main() {
    let args = Args::parse();
    println!("{args:?}");
}
