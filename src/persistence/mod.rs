use crate::error::Error;
use crate::model::Boss;

use async_trait::async_trait;

#[async_trait]
pub trait Persistence {
    type Error;

    async fn get_bosses(&self) -> Result<Vec<Boss>, Self::Error>;
    async fn save_bosses(&self, bosses: &[&Boss]) -> Result<(), Self::Error>;
}

pub struct JsonFile {
    path: String,
}

impl JsonFile {
    pub fn new(path: String) -> Self {
        Self { path }
    }

    pub fn path(&self) -> &str {
        self.path.as_ref()
    }
}

#[async_trait]
impl Persistence for JsonFile {
    type Error = Error;

    async fn get_bosses(&self) -> Result<Vec<Boss>, Self::Error> {
        let contents = tokio::fs::read(&self.path).await?;
        Ok(serde_json::from_slice(&contents)?)
    }

    async fn save_bosses(&self, bosses: &[&Boss]) -> Result<(), Self::Error> {
        let json = serde_json::to_string(bosses)?;
        Ok(tokio::fs::write(&self.path, &json).await?)
    }
}
