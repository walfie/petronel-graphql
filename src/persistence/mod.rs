use crate::error::Error;
use crate::model::Boss;

use async_trait::async_trait;
use redis::aio::ConnectionManager;
use redis::AsyncCommands;

#[async_trait]
pub trait Persistence {
    type Error;

    async fn get_bosses(&self) -> Result<Vec<Boss>, Self::Error>;
    async fn save_bosses(&self, bosses: &[&Boss]) -> Result<(), Self::Error>;
}

#[derive(Debug, Clone)]
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

#[derive(Clone)]
pub struct Redis {
    key: String,
    manager: ConnectionManager,
}

impl Redis {
    pub async fn new<T>(uri: T, key: String) -> redis::RedisResult<Self>
    where
        T: redis::IntoConnectionInfo,
    {
        let manager = ConnectionManager::new(uri.into_connection_info()?).await?;
        Ok(Self { manager, key })
    }
}

#[async_trait]
impl Persistence for Redis {
    type Error = Error;

    async fn get_bosses(&self) -> Result<Vec<Boss>, Self::Error> {
        let value: Option<Vec<u8>> = self.manager.clone().get(&self.key).await?;
        match value {
            None => Ok(Vec::new()),
            Some(contents) => Ok(serde_json::from_slice(&contents)?),
        }
    }

    async fn save_bosses(&self, bosses: &[&Boss]) -> Result<(), Self::Error> {
        let json = serde_json::to_string(bosses)?;
        Ok(self.manager.clone().set(&self.key, json).await?)
    }
}
