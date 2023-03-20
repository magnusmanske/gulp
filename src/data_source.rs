use crate::data_source_as_file::DataSourceAsFile;
use crate::data_source_line_converter::DataSourceLineConverter;
use crate::row::Row;
use crate::{GulpError, header::*, app_state::AppState};
use std::io::{BufReader, Seek, Read};
use std::sync::Arc;
use std::fs::File;
use mysql_async::prelude::*;
use serde::{Deserialize, Serialize};
use crate::cell::*;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DataSourceTypeUrl {}


#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DataSourceTypeFile {}


#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DataSourceTypePagePile {}




#[derive(Clone, Debug, Serialize)]
pub struct FileWithHeader {
    pub headers: Vec<HeaderColumn>,
    #[serde(skip)]
    pub file: Arc<File>,
}

#[derive(Clone, Debug, Serialize)]
pub struct CellSet {
    pub headers: Vec<HeaderColumn>,
    pub rows: Vec<Row>,
}

impl CellSet {
    pub fn get_cells_in_column(&self, column: usize) -> Vec<Cell> {
        self.rows
            .iter()
            .filter_map(|row|row.cells.get(column))
            .cloned()
            .filter_map(|c|c)
            .collect()
    }
}


#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DataSourceFormatJSONL {}

impl DataSourceFormatJSONL {
    pub fn guess_headers(&self, lines: &Vec<String>) -> Vec<HeaderColumn> {
        let columns = lines
            .iter()
            .filter_map(|line|serde_json::from_str::<serde_json::Value>(line).ok())
            .filter_map(|v|v.as_array().cloned())
            .map(|array|array.len())
            .max()
            .unwrap_or(0);
        let header = HeaderColumn{ column_type: ColumnType::String, wiki: None, string: None, namespace_id: None };
        std::iter::repeat(header).take(columns).collect()
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DataSourceFormatExcel {}

impl DataSourceFormatExcel {
    pub fn guess_headers(&self, lines: &Vec<String>) -> Vec<HeaderColumn> {
        let columns = lines
            .iter()
            .filter_map(|line|serde_json::from_str::<serde_json::Value>(line).ok())
            .filter_map(|v|v.as_array().cloned())
            .map(|array|array.len())
            .max()
            .unwrap_or(0);
        let header = HeaderColumn{ column_type: ColumnType::String, wiki: None, string: None, namespace_id: None };
        std::iter::repeat(header).take(columns).collect()
    }
}


#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DataSourceFormatPagePile {}

impl DataSourceFormatPagePile {
    pub fn get_lines_actually(&self, header_file: &mut FileWithHeader, limit: usize) -> Result<(String,Vec<String>),GulpError> {
        let file = Arc::get_mut(&mut header_file.file).unwrap();
        let mut reader = BufReader::new(file);
        let mut text = String::new();
        reader.read_to_string(&mut text)?;
        let json: serde_json::Value = serde_json::from_str(&text)?;
        let wiki = json.get("wiki").ok_or_else(||"import_from_pagepile: No field 'wiki'")?
            .as_str().ok_or_else(||"import_from_pagepile: field 'wiki' not a str")?.to_string();
        let iterator = json.get("pages").ok_or_else(||"import_from_pagepile: No field 'pages'")?
            .as_object().ok_or_else(||"import_from_pagepile: field 'pages' not an object")?
            .keys().take(limit).cloned();
        let lines = iterator.collect();
        reader.rewind()?;
        Ok((wiki,lines))
    }

}


#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DataSourceFormatCSV {}


#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DataSourceFormatTSV {}


#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum DataSourceFormat {
    TSV,
    CSV,
    JSONL,
    PAGEPILE,
    EXCEL,
}

impl DataSourceFormat {
    pub fn new(s: &str) -> Option<Self> {
        match s.trim().to_uppercase().as_str() {
            "CSV" => Some(Self::CSV),
            "TSV" => Some(Self::TSV),
            "JSONL" => Some(Self::JSONL),
            "PAGEPILE" => Some(Self::PAGEPILE),
            "XLS" => Some(Self::EXCEL),
            _ => None,
        }
    }

    pub fn to_string(&self) -> String {
        match self {
            Self::CSV => "CSV",
            Self::TSV => "TSV",
            Self::JSONL => "JSONL",
            Self::PAGEPILE => "PAGEPILE",
            Self::EXCEL => "XLS",
        }.to_string()
    }

    pub fn line_converter(&self) -> Arc<Box<dyn DataSourceLineConverter>> {
        Arc::new(match self {
            Self::CSV => Box::new(DataSourceFormatCSV {}),
            Self::TSV => Box::new(DataSourceFormatTSV {}),
            Self::JSONL => Box::new(DataSourceFormatJSONL {}),
            Self::PAGEPILE => Box::new(DataSourceFormatPagePile {}),
            Self::EXCEL => Box::new(DataSourceFormatExcel {}),
        })
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
            _ => None,
        }
    }

    pub fn to_string(&self) -> String {
        match self {
            Self::URL => "URL",
            Self::FILE => "FILE",
            Self::PAGEPILE => "PAGEPILE",
        }.to_string()
    }

    pub fn line_handler(&self) -> Arc<Box<dyn DataSourceAsFile>> {
        Arc::new(match self {
            Self::URL => Box::new(DataSourceTypeUrl{}),
            Self::FILE => Box::new(DataSourceTypeFile{}),
            Self::PAGEPILE => Box::new(DataSourceTypePagePile{}),
        })
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
        let sql = r#"SELECT id,list_id,source_type,source_format,location,user_id FROM data_source WHERE id=:source_id"#;
        app.get_gulp_conn().await.ok()?
            .exec_iter(sql,params! {source_id}).await.ok()?
            .map_and_drop(|row| Self::from_row(&row)).await.ok()?.get(0)?.to_owned()
    }

    pub async fn create(&mut self, app: &Arc<AppState>) -> Option<DbId> {
        let list_id = self.list_id;
        let source_type = self.source_type.to_string();
        let source_format = self.source_format.to_string();
        let location = self.location.to_owned();
        let user_id = self.user_id;
        let sql = "INSERT INTO `data_source` (list_id,source_type,source_format,location,user_id) VALUES (:list_id,:source_type,:source_format,:location,:user_id)";
        let mut conn = app.get_gulp_conn().await.ok()?;
        conn.exec_drop(sql, params!{list_id,source_type,source_format,location,user_id}).await.ok()?;
        self.id = conn.last_insert_id()?;
        Some(self.id)
    }

    pub async fn get_cells(&self, limit: Option<usize>) -> Result<CellSet,GulpError> {
        let mut header_file = self.get_line_set().await?;
        let lh = self.source_format.line_converter();
        lh.get_cells(&mut header_file, limit)
    }

    pub async fn guess_headers(&self, limit: Option<usize>) -> Result<CellSet,GulpError> {
        let mut header_file = self.get_line_set().await?;
        let cell_set = self.source_format.line_converter().get_cells(&mut header_file, limit)?;
        let headers = cell_set.headers.to_owned();
        let futures: Vec<_> = headers
            .iter()
            .enumerate()
            .map(|(column,header)|(cell_set.get_cells_in_column(column),header))
            .map(|(cells,header)|header.guess(cells))
            .collect();
        
        // Use new headers
        header_file.headers = futures::future::join_all(futures).await;
        let cell_set = self.source_format.line_converter().get_cells(&mut header_file, limit)?;

        Ok(cell_set)
    }


    async fn get_line_set(&self) -> Result<FileWithHeader,GulpError> {
        let lh = self.source_type.line_handler();
        Ok(FileWithHeader { headers: vec![], file: Arc::new(lh.as_file(self)?) })
    }

}