use crate::core::CoreCommand;
use clap::Parser;
use std::io;
use tokio::sync::mpsc;

#[derive(clap::Parser)]
pub(crate) struct ReplCommand {
    #[command(subcommand)]
    pub command: CoreCommand,
}

pub async fn run_loop(tx: mpsc::Sender<CoreCommand>) -> anyhow::Result<()> {
    println!("help для списка команд, exit для остановки");

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
