use anyhow::{Context, Result};
use futures::{Stream, StreamExt};
// use ipfs_api::IpfsApi;
use std::{io::Write, path::Path};
use tokio::io::{AsyncBufRead, AsyncBufReadExt};
use skynet_rs::{SkynetClient, UploadOptions, DownloadOptions, MetadataOptions};
use git2::Config;

use git_lfs_spec::transfer::custom::{self, Complete, Error, Event, Operation, Progress};

pub fn read_events(
    input: impl AsyncBufRead + AsyncBufReadExt + Unpin,
) -> impl Stream<Item = Result<Event>> {
    async_stream::stream! {
        let mut lines = input.lines();
        while let Some(line) = lines.next_line().await? {
            let parsed = serde_json::from_str(&line).context("could not parse JSON");
            yield parsed
        }
    }
}

const INTERNAL_SERVER_ERROR: i32 = 500;

pub fn transfer(
    client: SkynetClient,
    input_event_stream: impl Stream<Item = Result<Event>>,
    download_folder: impl AsRef<Path>,
) -> impl Stream<Item = Result<Event>> {
    let mut init_opt = None;

    async_stream::stream! {
        futures_util::pin_mut!(input_event_stream);

        // wait for json object messages to be sent to the process
        while let Some(event) = input_event_stream.next().await.transpose()? {
            match (init_opt.as_ref(), event) {
                (None, Event::Init(init)) => {
                    init_opt = Some(init);
                    yield Ok(Event::AcknowledgeInit)
                }
                (None, event) => {
                    yield Err(anyhow::anyhow!("Unexpected event: {:?}", event))
                }
                (Some(_), Event::Init(init)) => {
                    yield Err(anyhow::anyhow!("Unexpected init event: {:?}", init))
                }

                (Some(_), Event::Terminate) => {
                    break
                }
                (Some(init), event) => {
                    match (event, &init.operation) {

                        // download from skynet
                        (Event::Download(download), Operation::Download) => {
                            let oid = &download.object.oid;
                            // let cid_result = crate::ipfs::sha256_to_cid(&download.object.oid);
                            // match cid_result {
                            //     Ok(cid) => {
                            //         let output_path = download_folder.as_ref().join(&download.object.oid);
                            //         let mut output = std::fs::File::create(&output_path)?;
                            //
                            //         let mut stream =
                            //             client.block_get(&format!("/ipfs/{}", cid));
                            //         let mut bytes_so_far = 0;
                            //         while let Some(res) = stream.next().await {
                            //             let bytes = res?;
                            //             output.write_all(&bytes)?;
                            //             bytes_so_far += bytes.len() as u64;
                            //                 yield Ok(Event::Progress(
                            //                     Progress {
                            //                         oid: download.object.oid.clone(),
                            //                         bytes_so_far,
                            //                         bytes_since_last: bytes.len() as u64,
                            //                     }
                            //                     .into()
                            //                 ));
                            //         }
                            //         yield Ok(Event::Complete(
                            //             Complete {
                            //                 oid: download.object.oid.clone(),
                            //                 result: Some(custom::Result::Path(output_path)),
                            //             }
                            //             .into(),
                            //         ));
                            //     }
                            //     Err(err) => {
                            //         yield Ok(Event::Complete(
                            //             Complete {
                            //                 oid: download.object.oid.clone(),
                            //                 result: Some(custom::Result::Error(Error {
                            //                     code: INTERNAL_SERVER_ERROR,
                            //                     message: err.to_string(),
                            //                 })),
                            //             }
                            //             .into(),
                            //         ))
                            //     },
                            // }

                            todo!()
                        }

                        // upload to skynet
                        (Event::Upload(upload), Operation::Upload) => {
                            // open git config
                            let mut gitconf = git2::Config::open(&Path::new(".git/config"))
                                .expect("failed to open git config file");

                            // git object id
                            let oid = &upload.object.oid;

                            // the git config keys cannot start with numbers or git will say its invalid
                            let map_key = format!("lfs.customtransfer.skynet.mapping.oid-{}", oid);

                            // whether upload for the file is needed
                            let mut do_upload = true;

                            // mapping exists
                            if let Ok(base64Skylink) = gitconf.get_string(map_key.as_str()) {
                                // decode skylink
                                let skylink = base64::decode(base64Skylink).expect("failed to base64 decode skylink");
                                let skylink = String::from_utf8(skylink).expect("failed skylink parsing from git");

                                // check if file is still available
                                if let Ok(metadata) = client.get_metadata(skylink.as_str(), MetadataOptions::default()).await {
                                    // skip actual upload
                                    do_upload = false;

                                    yield Ok(Event::Complete(
                                        Complete {
                                            oid: upload.object.oid.clone(),
                                            result: None,
                                        }
                                        .into(),
                                    ))
                                }

                                // unset mapping
                                else {
                                    gitconf.remove(
                                        map_key.as_str(),
                                    );
                                }
                            }

                            // if no skylink mapping exists for the OID, or if the linked file is actually not available
                            if do_upload {
                                // upload file and get skylink
                                let uploadres = client.upload_file(
                                    &upload.path, UploadOptions{
                                        // api_key: client.options.api_key.clone(),
                                        ..Default::default()
                                    }
                                ).await;

                                if let Ok(skylink) = &uploadres {
                                    // save mapping
                                    gitconf.set_str(
                                        map_key.as_str(),
                                        base64::encode(skylink.replace("sia://", "")).as_str()
                                    ).expect("failed to write OID => Skylink mapping");

                                    // yield event for caller
                                    yield Ok(Event::Complete(
                                        Complete {
                                            oid: upload.object.oid.clone(),
                                            result: None,
                                        }
                                        .into(),
                                    ))
                                }

                                // error
                                else {
                                    yield Err(anyhow::anyhow!("There was an error trying to upload to skynet portal: {:?}", uploadres))
                                }
                            }
                        },

                        (event, _) => {
                            yield Err(anyhow::anyhow!("Unexpected event: {:?}", event))
                        },
                    };
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{fs::File, io::Read};

    use super::*;
    use crate::ipfs::client;
    use git_lfs_spec::{
        transfer::custom::{Download, Event, Init, Result, Upload},
        Object,
    };
    use pretty_assertions::assert_eq;
    use tempfile::tempdir;

    const FILE: &[u8] = b"hello world";
    const OID: &str = "b94d27b9934d3e08a52e52d7da7dabfac484efe37a5380ee9088f7ace2efcde9";
    const SIZE: u64 = FILE.len() as u64;

    #[tokio::test]
    async fn save_skylink_mapping() {
        // open git config
        let mut gitconf = git2::Config::open(&Path::new(".git/config"))
            .expect("failed to open git config file");
        // save mapping
        gitconf.set_str(
            format!("lfs.customtransfer.skynet.mapping.testoid").as_str(),
            "testskylink"
        ).expect("failed to write OID => Skylink mapping");
    }

    #[tokio::test]
    async fn read_events_parses_event_successfully() {
        let init = Event::Init(Init {
            operation: Operation::Download,
            remote: "origin".to_string(),
            concurrent: true,
            concurrenttransfers: Some(3),
        });
        let input: &[u8] = br#"{"event":"init","operation":"download","remote":"origin","concurrent":true,"concurrenttransfers":3}"#;
        let stream = read_events(input);
        futures::pin_mut!(stream);
        let mut events = vec![];
        while let Some(output) = stream.next().await {
            events.push(output.unwrap());
        }
        assert_eq!(events, &[init]);
    }

    #[tokio::test]
    #[ignore]
    async fn transfer_handles_events_as_expected_for_download() {
        let temp_dir = tempdir().unwrap();

        let expected_output_path = temp_dir.path().join(&OID);

        let client = client();
        let input_events = [
            Event::Init(Init {
                operation: Operation::Download,
                remote: "origin".to_string(),
                concurrent: true,
                concurrenttransfers: Some(3),
            }),
            Event::Download(
                Download {
                    object: Object {
                        oid: OID.to_string(),
                        size: SIZE,
                    },
                }
                .into(),
            ),
            Event::Terminate,
        ];
        let expected_output_events = [
            Event::AcknowledgeInit,
            Event::Progress(
                Progress {
                    oid: OID.to_string(),
                    bytes_so_far: SIZE,
                    bytes_since_last: SIZE,
                }
                .into(),
            ),
            Event::Complete(
                Complete {
                    oid: OID.to_string(),
                    result: Some(Result::Path(expected_output_path.clone())),
                }
                .into(),
            ),
        ];
        let output_stream = transfer(
            client,
            futures::stream::iter(input_events.iter().cloned().map(anyhow::Result::Ok)),
            temp_dir.path(),
        );
        futures_util::pin_mut!(output_stream);
        let expected_output_stream = futures::stream::iter(expected_output_events.iter().cloned());
        let mut actual_and_expected_output_stream = output_stream.zip(expected_output_stream);
        while let Some((actual, expected)) = actual_and_expected_output_stream.next().await {
            assert_eq!(actual.unwrap(), expected);
        }

        let mut actual_file = Vec::with_capacity(FILE.len());

        File::open(&expected_output_path)
            .unwrap()
            .read_to_end(&mut actual_file)
            .unwrap();

        assert_eq!(actual_file, FILE)
    }

    #[tokio::test]
    #[ignore]
    async fn transfer_handles_events_as_expected_for_upload() {
        let temp_dir = tempdir().unwrap();
        let temp_file = temp_dir.path().join(OID);
        std::fs::File::create(&temp_file).unwrap();

        let client = client();
        let input_events = [
            Event::Init(Init {
                operation: Operation::Upload,
                remote: "origin".to_string(),
                concurrent: true,
                concurrenttransfers: Some(3),
            }),
            Event::Upload(
                Upload {
                    object: Object {
                        oid: OID.to_string(),
                        size: SIZE,
                    },
                    path: temp_file.clone(),
                }
                .into(),
            ),
            Event::Terminate,
        ];
        let expected_output_events = [
            Event::AcknowledgeInit,
            Event::Complete(
                Complete {
                    oid: OID.to_string(),
                    result: None,
                }
                .into(),
            ),
        ];
        let output_stream = transfer(
            client,
            futures::stream::iter(input_events.iter().cloned().map(anyhow::Result::Ok)),
            temp_dir.path(),
        );
        futures_util::pin_mut!(output_stream);
        let expected_output_stream = futures::stream::iter(expected_output_events.iter().cloned());
        let mut actual_and_expected_output_stream = output_stream.zip(expected_output_stream);
        while let Some((actual, expected)) = actual_and_expected_output_stream.next().await {
            assert_eq!(actual.unwrap(), expected);
        }

        std::fs::remove_file(temp_file).unwrap();
    }
}
