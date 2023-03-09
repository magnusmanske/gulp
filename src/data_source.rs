use std::sync::Arc;
use mysql_async::prelude::*;
use serde::{Deserialize, Serialize};
use crate::GulpError;
use crate::{header::*, app_state::AppState};
use std::fs::File;
use std::io::{ self, BufRead};

pub trait DataSourceLineHandler {
    fn get_lines(&self, ds: &DataSource, limit: usize) -> Result<LineSet,GulpError> ;
}


#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DataSourceTypeUrl {}

impl DataSourceLineHandler for DataSourceTypeUrl {
    fn get_lines(&self, ds: &DataSource, limit: usize) -> Result<LineSet,GulpError> {
        let url = &ds.location;
        //let text = crate::list::List::get_client()?.get(url).send().await?.text().await?;
        let text = crate::list::List::get_text_from_url(&url)?;
        let lines : Vec<String> = text.split("\n").map(|s|s.to_string()).take(limit).collect();
        Ok(LineSet{ lines, headers: vec![] })
    }
}


#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DataSourceTypeFile {}

impl DataSourceLineHandler for DataSourceTypeFile {
    fn get_lines(&self, ds: &DataSource, limit: usize) -> Result<LineSet,GulpError> {
        let file = File::open(&ds.location).unwrap();
        let lines = io::BufReader::new(file).lines().filter_map(|row|row.ok()).take(limit).collect();
        Ok(LineSet{ lines, headers: vec![] })
    }
}


#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DataSourceTypePagePile {}

impl DataSourceLineHandler for DataSourceTypePagePile {
    fn get_lines(&self, ds: &DataSource, limit: usize) -> Result<LineSet,GulpError> {
        let id = ds.location.parse::<usize>()?;
        let url = format!("https://pagepile.toolforge.org/api.php?id={id}&action=get_data&doit&format=json&metadata=1");
        // let json: serde_json::Value = crate::list::List::get_client()?.get(url).send().await?.json().await?;
        let text = crate::list::List::get_text_from_url(&url)?;
        let json: serde_json::Value = serde_json::from_str(&text)?;
        let wiki = json.get("wiki").ok_or_else(||"import_from_pagepile: No field 'wiki'")?
            .as_str().ok_or_else(||"import_from_pagepile: field 'wiki' not a str")?.to_string();
        let header = HeaderColumn{ column_type: ColumnType::WikiPage, wiki:Some(wiki), string: None, namespace_id: None };
        let lines = json.get("pages").ok_or_else(||"import_from_pagepile: No field 'pages'")?
            .as_object().ok_or_else(||"import_from_pagepile: field 'pages' not an object")?
            .keys().take(limit).cloned().collect();
        Ok(LineSet{ lines, headers: vec![Some(header)] })
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct LineSet {
    pub headers: Vec<Option<crate::header::HeaderColumn>>,
    pub lines: Vec<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct CellStringSet {
    pub headers: Vec<Option<crate::header::HeaderColumn>>,
    pub rows: Vec<Vec<String>>,
}


// #[derive(Clone, Debug, Serialize)]
// pub struct CellRowSet {
//     pub headers: Vec<Option<crate::header::HeaderColumn>>,
//     pub rows: Vec<Vec<crate::cell::Cell>>,
// }

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
            "CSV" => Some(Self::CSV),
            "TSV" => Some(Self::TSV),
            "JSONL" => Some(Self::JSONL),
            "PAGEPILE" => Some(Self::PAGEPILE),
            _ => None,
        }
    }

    pub fn to_string(&self) -> String {
        match self {
            Self::CSV => "CSV",
            Self::TSV => "TSV",
            Self::JSONL => "JSONL",
            Self::PAGEPILE => "PAGEPILE",
        }.to_string()
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

    pub fn line_handler(&self) -> Arc<Box<dyn DataSourceLineHandler>> {
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
        let sql = r#"SELECT * FROM data_source WHERE id=:source_id"#;
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

    pub async fn get_string_cells(&self, limit: Option<usize>) -> Result<CellStringSet,GulpError> {
        let line_set = self.get_lines(limit).await?;
        let rows = line_set.lines
            .iter()
            .filter_map(|line|self.strings_from_line(line,&line_set.headers))
            .collect();
        Ok(CellStringSet{headers:line_set.headers, rows})
    }

    fn strings_from_line(&self, _line: &str,_headers: &Vec<Option<HeaderColumn>>) -> Option<Vec<String>> {
        todo!()
    }

    // pub async fn get_cell_rows(&self, limit: Option<usize>) -> Result<CellRowSet,GulpError> {
    //     let line_set = self.get_lines(limit).await?;
    //     let rows = line_set.lines
    //         .iter()
    //         .filter_map(|line|self.cell_from_line(line,&line_set.headers))
    //         .collect();
    //     Ok(CellRowSet{headers:line_set.headers, rows})
    // }

    // fn cell_from_line(&self, line: &str, headers: &Vec<Option<HeaderColumn>>) -> Option<Vec<crate::cell::Cell>> {
    //     todo!()
    // }

    pub async fn get_lines(&self, limit: Option<usize>) -> Result<LineSet,GulpError> {
        let limit = limit.unwrap_or(usize::MAX);
        let lh = self.source_type.line_handler();
        lh.get_lines(&self, limit)
    }

}