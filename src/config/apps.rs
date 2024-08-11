use serde::Deserialize;
use std::{collections::HashMap, path::PathBuf};

use super::app::{AppConfig, AppConfigRaw};

#[derive(Debug, Clone)]
pub struct Apps {
    pub apps: HashMap<String, AppConfig>,
    pub certs: Vec<PathBuf>,
}

impl Apps {
    pub fn new(data: String) -> Result<Self, serde_json::Error> {
        Ok(Self::from_raw(AppsRaw::new(data)?))
    }
    pub fn from_raw(data: AppsRaw) -> Self {
        let mut apps = HashMap::new();
        for (name, config) in data.apps {
            apps.insert(name.clone(), AppConfig::from_raw(config, name));
        }
        Apps {
            apps,
            certs: data.certs,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct AppsRaw {
    pub apps: HashMap<String, AppConfigRaw>,
    pub certs: Vec<PathBuf>,
}

impl AppsRaw {
    pub fn new(data: String) -> Result<Self, serde_json::Error> {
        serde_json::from_str(&data)
    }
}
