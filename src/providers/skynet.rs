use std::{env, io::Write, path::Path};
// use tokio::io::{AsyncBufRead, AsyncBufReadExt};
use std::fs::File;
use skynet_rs::{SkynetClient, UploadOptions, DownloadOptions, MetadataOptions, SkynetClientOptions, SkynetError};
use std::cmp::min;
use std::sync::Arc;
use anyhow::{Context, Result};
// use async_stream::AsyncStream;
use git_lfs_spec::transfer::custom::{Complete, Download, Event, Progress, Upload};
use async_trait::async_trait;
use futures::{Stream, StreamExt};
use git_lfs_spec::Object;
use git_lfs_spec::transfer::custom;
use reqwest::Client;
use futures_util::pin_mut;
use isahc::{ReadResponseExt, RequestExt};

use crate::provider::{BoxedStream, StorageProvider};

#[derive(Copy, Clone, Debug)]
pub enum UploadStrategy {
    Client,
    CURL,
}

#[derive(Clone)]
pub struct SkynetProvider {
    client: Arc<SkynetClient>,
    pub strategy: UploadStrategy,
}

impl SkynetProvider {
    pub fn new_from_env(strategy: UploadStrategy) -> Result<Self> {
        let mut env_variables
            = env_file_reader::read_file(".skynet.env")?;

        log::debug!("skynet vars: {:#?}", &env_variables);

        let portal_url = env_variables.remove("SKYNET_PORTAL_URL")
            .unwrap_or("https://skynetfree.net".to_string());

        log::debug!("using Skynet portal: {}", &portal_url);

        let client = SkynetClient::new(portal_url.as_str(), SkynetClientOptions {
            api_key: env_variables.remove("SKYNET_API_KEY"),
            custom_user_agent: None,
        });

        Ok(Self {
            client: client.into(),
            strategy,
        })
    }

    fn git_map_key(oid: &String) -> String {
        format!("lfs.customtransfer.skynet.mapping.oid-{}", oid)
    }

    fn git_save_mapping(oid: &String, skylink: &String) {
        let mut gitconf = Self::git_config();
        gitconf.set_str(
            Self::git_map_key(oid).as_str(),
            base64::encode(skylink.replace("sia://", "")).as_str(),
        ).expect("failed to write OID => Skylink mapping");
    }

    fn get_skylink(oid: &String) -> Option<String> {
        Self::git_config()
            .get_string(Self::git_map_key(oid).as_str())
            .ok()
            .map(|skylinkb64| String::from_utf8(base64::decode(skylinkb64).unwrap()).expect("parse skylink bytes to string"))
    }

    // todo: return response
    async fn upload_isahc(&self, upload: &Upload) -> Result<()> {
        let file = std::fs::File::open(upload.path.clone())?;

        log::debug!("instantiated file object");

        let endpoint = format!("{}/skynet/skyfile/{}",
                               self.client.get_portal_url(),
                               &upload.object.oid);

        // Perform the upload.
        let mut response = isahc::Request::post(endpoint)
            // todo: we have hardcoded the api key expectation here
            .header("Skynet-Api-Key", self.client.get_options().api_key.clone().unwrap())
            .header("Content-Type", "multipart/form-data")
            // todo: set file to the 'file' form field
            .body(file)?
            .send();

        log::debug!("passed http call");

        if response.is_err() {
            return Err(anyhow::anyhow!("There was an error trying to upload to skynet portal: {:?}", response));
        } else if let Ok(mut resp) = response {
            let skylink = resp.text()?;

            log::debug!("upload complete: {}", &skylink);

            // save mapping
            Self::git_save_mapping(&upload.object.oid, &skylink);
        }

        Ok(())
    }

    async fn upload_skynet_rs(&self, upload: &Upload) -> Result<()> {
        // upload file and get skylink
        let uploadres = self.client.upload_file(
            &upload.path,
            UploadOptions {
                // api_key: client.options.api_key.clone(),
                ..Default::default()
            },
        ).await;

        if let Ok(skylink) = &uploadres {
            log::debug!("upload complete: {}", &skylink);

            // save mapping
            Self::git_save_mapping(&upload.object.oid, skylink);

            return Ok(());
        }

        // error
        else {
            Err(anyhow::anyhow!("There was an error trying to upload to skynet portal: {:?}", uploadres))
        }
    }
}

#[async_trait]
impl StorageProvider for SkynetProvider {
    async fn download(&self, download: &Download) -> Result<Complete> {
        match Self::get_skylink(&download.object.oid) {
            Some(skylink) => {
                let output_path = format!("/tmp/{}", &download.object.oid);

                self
                    .client
                    .download_file(&output_path, &skylink, DownloadOptions::default())
                    .await
                    .map(|_| Complete{
                        oid: download.object.oid.clone(),
                        result: Some(custom::Result::Path(output_path.into()))
                    })
                    .map_err(|skynet_err| anyhow::anyhow!("an error occurred in skynet-rs: {:#?}", skynet_err))
            }

            // no skylink found in mapping
            None => {
                Err(anyhow::anyhow!("no skylink found in .git/config mapping for {}", &download.object.oid))
            }
        }
    }

    fn download_stream(self, download: Download) -> BoxedStream where Self: 'static {
        let stream = async_stream::stream! {
            match Self::get_skylink(&download.object.oid) {
                Some(skylink) => {
                    let output_path = format!("/tmp/{}", &download.object.oid);
                    std::fs::remove_file(&output_path);

                    let mut output_event_stream = self
                        .client
                        .download_file_stream(
                            output_path.clone(),
                            &skylink,
                            DownloadOptions::default()
                        );

                    futures_util::pin_mut!(output_event_stream);

                    let mut last_downloaded = 0;

                    while let Some(downloaded) = output_event_stream.next().await.transpose().expect("stream error") {
                        let since_last = downloaded - last_downloaded;

                        yield Ok(Event::Progress(Progress {
                            oid: download.object.oid.clone(),
                            bytes_so_far: downloaded,
                            bytes_since_last: since_last
                        }.into(),));

                        last_downloaded = downloaded;
                    }

                    // todo: yield error if expected file size doesnt match?
                    yield Ok(Event::Complete(
                        Complete {
                            oid: download.object.oid.clone(),
                            result: Some(custom::Result::Path(output_path.into())),
                        }
                        .into(),
                    ))
                }

                // no skylink found in mapping
                None => {
                    yield Err(anyhow::anyhow!("no skylink found in .git/config mapping for {}", &download.object.oid))
                }
            }
        };

        stream.boxed()
    }

    async fn upload(&self, upload: &Upload) -> Result<()> {
        log::debug!("received request to upload: {:#?}", &upload);

        // git object id
        let oid = &upload.object.oid;

        // if no skylink mapping exists for the OID, or if the linked file is actually not available
        log::debug!("uploading {}...", &upload.path.display());

        match self.strategy {
            UploadStrategy::Client => {
                self.upload_skynet_rs(upload).await
            }
            UploadStrategy::CURL => {
                self.upload_isahc(upload).await
            }
        }
    }

    async fn is_uploaded(&self, upload: &Upload) -> Result<bool> {
        // git object id
        let oid = &upload.object.oid;

        // the git config keys cannot start with numbers or git will say its invalid
        let map_key = Self::git_map_key(oid);

        // mapping exists
        if let Some(skylink) = Self::get_skylink(oid) {
            log::debug!("found OID => skylink mapping in git config");

            // log::debug!("checking if file for Skylink {} is still available...", &skylink);

            // check if file is still available
            if let Ok(metadata) = self.client.get_metadata(skylink.as_str(), MetadataOptions::default()).await {
                return Ok(true);
            }

            // unset mapping
            log::debug!("file no longer available. Unsetting mapping");

            // open git config
            let mut gitconf = Self::git_config();

            gitconf.remove(
                map_key.as_str(),
            );
        }

        Ok(false)
    }
}

#[tokio::test]
async fn save_skylink_mapping() {
    // open git config
    let mut gitconf = git2::Config::open(&Path::new(".git/config"))
        .expect("failed to open git config file");
    // save mapping
    gitconf.set_str(
        format!("lfs.customtransfer.skynet.mapping.testoid").as_str(),
        "testskylink",
    ).expect("failed to write OID => Skylink mapping");
}

#[tokio::test]
async fn stream_download() {
    let oid = "6f1eb46bfea22b6facfb12c583e61f699232cbed05d4f05a9cb314f634e88d9d".to_string();

    let client = SkynetProvider::new_from_env(UploadStrategy::Client).unwrap();

    // save skylink mapping
    SkynetProvider::git_save_mapping(&oid, &"AABC5fIelZsChCGs-fSBRVc5n2BoHc-LAmehPlPRBjIV9w".to_string());

    // the output stream we are writing back to the console for git-lfs to read
    let mut output_event_stream = client.download_stream(Download {
        object: Object {
            oid: oid,
            size: 0
        }
    });

    // futures_util::pin_mut!(output_event_stream);

    let mut last_event = None;

    while let Some(output_event) = output_event_stream.next().await.transpose().expect("stream error") {
        dbg!(&output_event);

        last_event = Some(output_event);
    }

    // todo check file size
    // assert_eq!();
}

#[tokio::test]
async fn test_isahc_upload() {
    assert!(SkynetProvider::new_from_env(UploadStrategy::CURL)
        .unwrap()
        .upload_isahc(&Upload {
            object:
            Object {
                oid: "67da1154a858aa2d89f5201246f6d4b43a0a4eb55011136e993728f0daf8703e".to_string(),
                size: 0,
            },
            path: Default::default(),
        })
        .await
        .is_ok())
}