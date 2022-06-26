use std::future::Future;
use std::path::Path;
use std::pin::Pin;
use anyhow::{Context, Result};
use async_stream::AsyncStream;
use git_lfs_spec::transfer::custom::*;
use async_trait::async_trait;
use futures::{Stream, StreamExt};

pub type BoxedStream = Pin<Box<Stream<Item = anyhow::Result<Event>>>>;

#[async_trait]
pub trait StorageProvider : Sync + Sized + Send + Clone {
    async fn download(&self, obj: &Download) -> anyhow::Result<Complete>;

    fn download_stream(self, download: Download) -> BoxedStream where Self: 'static {
        let stream = async_stream::stream! {
            yield self
                .download(&download)
                .await
                .map(|cmpl|
                    Event::Complete(cmpl.into()))
        };
        stream.boxed()
    }

    async fn upload(&self, obj: &Upload) -> anyhow::Result<()>;

    fn upload_stream(self, upload: Upload) -> BoxedStream where Self: 'static {
        let stream = async_stream::stream! {
            yield self
                .upload(&upload)
                .await
                .map(|_|
                    Event::Complete(Complete {
                        oid: upload.object.oid.clone(),
                        result: None
                    }.into())
                )
        };
        stream.boxed()
    }

    async fn is_uploaded(&self, obj: &Upload) -> anyhow::Result<bool>;

    async fn upload_if_needed(&self, obj: &Upload) -> anyhow::Result<()> {
        match self.is_uploaded(obj).await {
            Ok(true) => {
                Ok(())
            }
            _ => self.upload(obj).await
        }
    }

    fn git_config() -> git2::Config {
        // todo: make configurable.. env var?
        git2::Config::open(&Path::new(".skynetlfsconfig"))
            .expect("failed to open git config file")
    }
}