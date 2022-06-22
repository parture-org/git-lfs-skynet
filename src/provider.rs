use std::future::Future;
use std::path::Path;
use anyhow::{Context, Result};
use git_lfs_spec::transfer::custom::*;
use async_trait::async_trait;

#[async_trait]
pub trait StorageProvider : Sync {
    async fn download(&self, obj: &Download) -> anyhow::Result<String>;
    async fn upload(&self, obj: &Upload) -> anyhow::Result<()>;
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
        git2::Config::open(&Path::new(".git/config"))
            .expect("failed to open git config file")
    }
}