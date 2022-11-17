use std::path::Path;
use git_lfs_spec::transfer::custom::{Complete, Download, Upload};
use anyhow::{Context, Result};
use async_trait::async_trait;
use crate::provider::StorageProvider;

use s3::bucket::Bucket;
use s3::creds::Credentials;
use s3::region::Region;

#[derive(Clone)]
pub struct StorJProvider {
    pub bucket: Bucket
}

impl StorJProvider {
    pub fn new_from_env() -> Result<Self> {
        log::debug!("parsing storj env vars...");

        let mut env_variables
            = env_file_reader::read_file(".storj.env")
                .context("reading .storj.env file")?;

        log::debug!("storj vars: {:#?}", &env_variables);

        Ok(Self {
            bucket: Bucket::new(
                // todo: make configurable
                &"parture-dev1".to_string(),
                Region::Custom {
                    region: env_variables.remove("STORJ_S3_REGION").unwrap_or("eu1".to_string()),
                    endpoint: env_variables.remove("STORJ_S3_ENDPOINT").unwrap_or("https://gateway.storjshare.io".to_string())
                },
                Credentials::new(
                    env_variables.get("STORJ_S3_ACCESS_KEY").map(String::as_str),
                    env_variables.get("STORJ_S3_SECRET").map(String::as_str),
                    None,
                    None,
                    None
                )?
            )?
        })
    }

    fn object_path(oid: &String) -> String {
        format!("/{}", oid)
    }
}

impl Default for StorJProvider {
    fn default() -> Self {
        Self::new_from_env().unwrap()
    }
}

#[async_trait]
impl StorageProvider for StorJProvider {
    async fn download(&self, download: &Download) -> anyhow::Result<Complete> {
        // todo: generate temp file
        let mut async_output_file = tokio::fs::File::create("/tmp/")
            .await
            .expect("Unable to create file");

        // Async variant with `tokio` or `async-std` features
        let status_code = self.bucket.get_object_stream(
            Self::object_path(&download.object.oid),
            &mut async_output_file
        ).await?;

        todo!()
    }

    async fn upload(&self, upload: &Upload) -> anyhow::Result<()> {
        log::debug!("received request to upload: {:#?}", &upload);

        let oid = &upload.object.oid;
        let objpath = StorJProvider::object_path(oid);

        log::debug!("uploading {}...", &upload.path.display());

        let mut file = tokio::fs::File::open(upload.path.clone()).await.context("opening File to upload")?;

        let status_code = self
            .bucket
            .put_object_stream(&mut file, objpath)
            .await
            .context("uploading object")?;

        if status_code < 300 {
            log::debug!("upload complete: {}", &oid);

            return Ok(())
        }

        // error
        Err(anyhow::anyhow!("There was an error trying to upload to storj portal: {:?}", status_code))
    }

    async fn is_uploaded(&self, obj: &Upload) -> anyhow::Result<bool> {
        // check bucket to see whether file exists and length is the same
        let (head_object_result, code) = self
            .bucket
            .head_object(StorJProvider::object_path(&obj.object.oid))
            .await
            .context("retrieving HEAD object info")?;

        // todo: check file len
        // head_object_result.content_length == Some(upload.object.len)

        // whether upload for the file is needed
        Ok(code < 300)
    }
}

#[tokio::test]
async fn test_storj_bucket_ls() {
    let bucket = StorJProvider::default().bucket;

    dbg!(bucket.list("/".to_string(), Some("/".to_string())).await.unwrap());
}

#[tokio::test]
async fn test_storj_bucket_head() {
    let bucket = StorJProvider::default().bucket;

    dbg!(bucket.head_object("021f5413c192dfd9932cac2279a48fcd4002b1ede8b997b09a40c5454a3cc06b").await.unwrap());
}

#[tokio::test]
async fn test_storj_bucket_upload() {
    let bucket = StorJProvider::default().bucket;

    let mut file = tokio::fs::File::open(".git/lfs/objects/67/da/67da1154a858aa2d89f5201246f6d4b43a0a4eb55011136e993728f0daf8703e").await.unwrap();

    bucket.put_object_stream(&mut file, "/67da1154a858aa2d89f5201246f6d4b43a0a4eb55011136e993728f0daf8703e").await.unwrap();
}

#[tokio::test]
async fn test_storj_bucket_delete() {
    let bucket = StorJProvider::default().bucket;

    bucket.delete_object("/67da1154a858aa2d89f5201246f6d4b43a0a4eb55011136e993728f0daf8703e").await;
}