use crate::core::CoreCommand;
use clap::{CommandFactory, Parser};
use std::io;
use tokio::sync::mpsc;

#[derive(clap::Parser)]
#[command(
    no_binary_name = true,
    disable_help_flag = true,
    subcommand_required = true
)]
pub(crate) struct ReplCommand {
    #[command(subcommand)]
    pub command: CoreCommand,
}

fn print_help() {
    let mut cmd = ReplCommand::command();
    let help = cmd.render_help();
    println!("{help}");
}

pub async fn run_loop(tx: mpsc::Sender<CoreCommand>) -> anyhow::Result<()> {
    print_help();
    println!("Type 'help' for the list of commands and 'exit' to stop.");
    println!();

    let mut line = String::new();
    loop {
        line.clear();
        let read = io::stdin().read_line(&mut line)?;
        if read == 0 {
            tx.send(CoreCommand::Exit).await?;
            break;
        }

        let input = line.trim();
        if input.is_empty() {
            continue;
        }

        if input.eq_ignore_ascii_case("exit") {
            tx.send(CoreCommand::Exit).await?;
            break;
        }

        if input.eq_ignore_ascii_case("help") {
            print_help();
            println!("Type 'exit' to stop.");
            println!();
            continue;
        }

        let mut argv = vec!["germina".to_string()];
        argv.extend(input.split_whitespace().map(ToOwned::to_owned));

        match ReplCommand::try_parse_from(argv) {
            Ok(cmd) => {
                if let Err(err) = tx.send(cmd.command).await {
                    eprintln!("Failed to deliver command: {err}");
                    break;
                }
            }
            Err(err) => {
                eprintln!("{err}");
            }
        }
    }

    Ok(())
}
