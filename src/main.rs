use std::env;
use anyhow::Result;
use futures::StreamExt;
use git_lfs_spec::transfer::custom::Event;
use std::path::PathBuf;
use skynet_rs::{SkynetClient, SkynetClientOptions};
use structopt::StructOpt;
use tokio::io::{stdin, stdout, BufReader};

use log::LevelFilter;
use log4rs::append::file::FileAppender;
use log4rs::encode::pattern::PatternEncoder;
use log4rs::config::{Appender, Config, Root};

use crate::providers::{SkynetProvider, StorJProvider, UploadStrategy};

// use crate::{clean::clean, smudge::smudge};

// mod clean;
// mod smudge;
mod transfer;
mod provider;
mod providers;

#[derive(Debug, StructOpt)]
#[structopt(author, about)]
enum GitLfsIpfs {
    /// git-lfs smudge filter extension for IPFS
    ///
    /// https://github.com/git-lfs/git-lfs/blob/main/docs/extensions.md#smudge
    // Smudge {
    //     /// Name of the file
    //     filename: PathBuf,
    // },
    /// git-lfs clean filter extension for IPFS
    ///
    /// <https://github.com/git-lfs/git-lfs/blob/main/docs/extensions.md#clean>
    // Clean {
    //     /// Name of the file
    //     filename: PathBuf,
    // },
    /// git-lfs custom transfer for IPFS
    ///
    /// <https://github.com/git-lfs/git-lfs/blob/main/docs/custom-transfers.md>
    Transfer,
}

#[tokio::main]
async fn main() -> Result<()> {
    main_inner().await.map_err(|err| {
        log::error!("an error occurred: {}", err);
        err
    })
}

async fn main_inner() -> Result<()> {
    let logfile = FileAppender::builder()
        .encoder(Box::new(PatternEncoder::new("{l} - {m}\n")))
        .build("log/output.log")?;

    let config = Config::builder()
        .appender(Appender::builder().build("logfile", Box::new(logfile)))
        .build(Root::builder()
            .appender("logfile")
            .build(LevelFilter::Debug))?;

    log4rs::init_config(config)?;

    log::debug!("pwd: {}", env::current_dir().unwrap().display());

    // todo: dynamic client choice here based on ENV var?
    // let client = StorJProvider::new_from_env()?;
    let client = SkynetProvider::new_from_env(UploadStrategy::Client)?;

    match GitLfsIpfs::from_args() {
        // GitLfsIpfs::Smudge { filename: _ } => smudge(client, stdin(), stdout()).await,
        // GitLfsIpfs::Clean { filename: _ } => clean(client, std::io::stdin(), stdout()).await,
        GitLfsIpfs::Transfer => {
            let buffered_stdin = BufReader::new(stdin());

            // the input stream of events passed by git-lfs
            let input_event_stream = transfer::read_events(buffered_stdin);

            // the output stream we are writing back to the console for git-lfs to read
            let output_event_stream =
                transfer::transfer(client, input_event_stream);

            futures_util::pin_mut!(output_event_stream);

            while let Some(output_event) = output_event_stream.next().await.transpose()? {
                if Event::AcknowledgeInit == output_event {
                    println!("{{ }}");
                } else {
                    let json = serde_json::to_string(&output_event)?;
                    log::debug!("emitting event to stdout stream: {}", &json);
                    println!("{}", json);
                }
            }
            Ok(())
        }
    }
}
