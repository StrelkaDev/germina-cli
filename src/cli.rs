use crate::core::{CoreCommand, CoreRequest};
use clap::{CommandFactory, Parser};
use std::io;
use tokio::sync::{mpsc, oneshot};

#[derive(clap::Parser)]
#[command(
    no_binary_name = true,
    disable_help_flag = true,
    subcommand_required = true,
    override_usage = "<COMMAND>"
)]
pub(crate) struct ReplCommand {
    #[command(subcommand)]
    pub command: CoreCommand,
}

enum ParsedInput {
    Exit,
    Help,
    Command(CoreCommand),
}

fn print_help() {
    let mut cmd = ReplCommand::command();
    let help = cmd.render_help();
    println!("{help}");
}

fn parse_input(input: &str) -> Result<ParsedInput, clap::Error> {
    if input.eq_ignore_ascii_case("exit") {
        return Ok(ParsedInput::Exit);
    }

    if input.eq_ignore_ascii_case("help") {
        return Ok(ParsedInput::Help);
    }

    let argv = input.split_whitespace();
    ReplCommand::try_parse_from(argv).map(|cmd| ParsedInput::Command(cmd.command))
}

async fn dispatch_command(
    tx: &mpsc::Sender<CoreRequest>,
    command: CoreCommand,
) -> anyhow::Result<()> {
    let (completion_tx, completion_rx) = oneshot::channel();

    tx.send(CoreRequest {
        command,
        completion_tx,
    })
    .await
    .map_err(|err| anyhow::anyhow!("Failed to deliver command: {err}"))?;

    match completion_rx.await {
        Ok(result) => result,
        Err(err) => Err(anyhow::anyhow!(
            "Failed to receive command completion: {err}"
        )),
    }
}

pub async fn run_loop(tx: mpsc::Sender<CoreRequest>) -> anyhow::Result<()> {
    print_help();
    println!("Type 'help' for the list of commands and 'exit' to stop.");
    println!();

    let mut line = String::new();
    loop {
        line.clear();
        let read = io::stdin().read_line(&mut line)?;
        if read == 0 {
            break;
        }

        let input = line.trim();
        if input.is_empty() {
            continue;
        }

        match parse_input(input) {
            Ok(ParsedInput::Exit) => break,
            Ok(ParsedInput::Help) => {
                print_help();
                println!("Type 'exit' to stop.");
                println!();
            }
            Ok(ParsedInput::Command(command)) => {
                if let Err(err) = dispatch_command(&tx, command).await {
                    eprintln!("{err}");
                    break;
                }
            }
            Err(err) => eprintln!("{err}"),
        }
    }

    Ok(())
}
