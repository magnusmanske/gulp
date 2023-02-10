use serde::{Deserialize, Serialize};
use crate::header::*;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum DataSourceFormat {
    TSV,
    CSV,
    JSONL,
}

impl DataSourceFormat {
    pub fn new(s: &str) -> Option<Self> {
        match s.trim().to_uppercase().as_str() {
            "TSV" => Some(Self::TSV),
            "CSV" => Some(Self::CSV),
            "JSONL" => Some(Self::JSONL),
            _ => None
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum DataSourceType {
    URL,
    FILE,
}

impl DataSourceType {
    pub fn new(s: &str) -> Option<Self> {
        match s.trim().to_uppercase().as_str() {
            "URL" => Some(Self::URL),
            "FILE" => Some(Self::FILE),
            _ => None
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DataSource {
    pub id: DbId,
    pub list_id: DbId,
    pub source_type: DataSourceType,
    pub source_format: DataSourceFormat,
    pub location: String,
    pub user_id: DbId,
}

impl DataSource {
    pub fn from_row(row: &mysql_async::Row) -> Option<Self> {
        let source_type: String = row.get(2)?;
        let source_format: String = row.get(3)?;
        Some(Self {
            id: row.get(0)?,
            list_id: row.get(1)?,
            source_type: DataSourceType::new(&source_type)?,
            source_format: DataSourceFormat::new(&source_format)?,
            location: row.get(4)?,
            user_id: row.get(5)?,
        })
    }
}