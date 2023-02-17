use std::sync::Arc;
use mysql_async::prelude::*;
use serde::{Deserialize, Serialize};
use crate::{header::*, app_state::AppState};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum DataSourceFormat {
    TSV,
    CSV,
    JSONL,
    PAGEPILE,
}

impl DataSourceFormat {
    pub fn new(s: &str) -> Option<Self> {
        match s.trim().to_uppercase().as_str() {
            "TSV" => Some(Self::TSV),
            "CSV" => Some(Self::CSV),
            "JSONL" => Some(Self::JSONL),
            "PAGEPILE" => Some(Self::PAGEPILE),
            _ => None
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum DataSourceType {
    URL,
    FILE,
    PAGEPILE,
}

impl DataSourceType {
    pub fn new(s: &str) -> Option<Self> {
        match s.trim().to_uppercase().as_str() {
            "URL" => Some(Self::URL),
            "FILE" => Some(Self::FILE),
            "PAGEPILE" => Some(Self::PAGEPILE),
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

    pub async fn from_db(app: &Arc<AppState>, source_id: DbId) -> Option<Self> {
        let sql = r#"SELECT * FROM data_source WHERE id=:source_id"#;
        app.get_gulp_conn().await.ok()?
            .exec_iter(sql,params! {source_id}).await.ok()?
            .map_and_drop(|row| Self::from_row(&row)).await.ok()?.get(0)?.to_owned()
    }
}