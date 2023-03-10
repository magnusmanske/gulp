use crate::{GulpError, header::*, app_state::AppState};
use std::sync::Arc;
use std::fs::File;
use std::io::{ self, BufRead};
use mysql_async::prelude::*;
use serde::{Deserialize, Serialize};
use crate::cell::*;

/// Trait for converting various data sources into a LineSet
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
        let lines : Vec<String> = text.split("\n").map(|s|s.trim().to_string()).take(limit).collect();
        Ok(LineSet{ lines, headers: vec![] })
    }
}


#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DataSourceTypeFile {}

impl DataSourceLineHandler for DataSourceTypeFile {
    fn get_lines(&self, ds: &DataSource, limit: usize) -> Result<LineSet,GulpError> {
        let file = File::open(&ds.location).unwrap();
        let lines = io::BufReader::new(file).lines().filter_map(|row|row.ok()).map(|s|s.trim().to_string()).take(limit).collect();
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
        Ok(LineSet{ lines, headers: vec![header] })
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct LineSet {
    pub headers: Vec<HeaderColumn>,
    pub lines: Vec<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct CellSet {
    pub headers: Vec<HeaderColumn>,
    pub rows: Vec<Vec<Option<Cell>>>,
}

impl CellSet {
    pub fn get_cells_in_column(&self, column: usize) -> Vec<Cell> {
        self.rows
            .iter()
            .filter_map(|row|row.get(column))
            .cloned()
            .filter_map(|c|c)
            .collect()
    }
}

/// Trait for converting LineSet via various formats into CellSet
pub trait DataSourceLineConverter {
    fn get_cells(&self, line_set: &LineSet) -> Result<CellSet,GulpError> ;
}


#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DataSourceFormatJSONL {}

impl DataSourceFormatJSONL {
    fn guess_headers(&self, lines: &Vec<String>) -> Vec<HeaderColumn> {
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

impl DataSourceLineConverter for DataSourceFormatJSONL {
    fn get_cells(&self, line_set: &LineSet) -> Result<CellSet,GulpError> {
        let mut headers = line_set.headers.to_owned();
        if headers.is_empty() {
            headers = self.guess_headers(&line_set.lines);
        }
        let mut rows: Vec<_> = vec!();
        for line in &line_set.lines {
            let json: serde_json::Value = serde_json::from_str(line)?;
            let array = json.as_array().ok_or_else(||"import_jsonl: valid JSON but not an array: {row}")?;
            let cells: Vec<Option<Cell>> = array
                .iter()
                .zip(headers.iter())
                .map(|(value,column)|Cell::from_value(value, column))
                .collect();
            rows.push(cells);
        }
        Ok(CellSet{headers:headers.to_owned(),rows})
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DataSourceFormatPagePile {}

impl DataSourceLineConverter for DataSourceFormatPagePile {
    fn get_cells(&self, line_set: &LineSet) -> Result<CellSet,GulpError> {
        let header = line_set.headers.get(0).ok_or_else(||"PagePile line_set has no headers")?;
        let wiki = header.wiki.as_ref().ok_or_else(||"PagePile first header has no wiki")?;
        let api = AppState::get_api_for_wiki(wiki)?;
        let rows: Vec<_> = line_set.lines
            .iter()
            .map(|line|wikibase::mediawiki::title::Title::new_from_full(&line, &api))
            .map(|title|WikiPage{ title: title.pretty().to_string(), namespace_id: Some(title.namespace_id()), wiki: Some(wiki.to_owned()) })
            .map(|page|vec![Some(Cell::WikiPage(page))])
            .collect();
        Ok(CellSet{headers:line_set.headers.to_owned(),rows})
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DataSourceFormatCSV {}

impl DataSourceLineConverter for DataSourceFormatCSV {
    fn get_cells(&self, line_set: &LineSet) -> Result<CellSet,GulpError> {
        DataSource::get_cells_xsv(b',', line_set)
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DataSourceFormatTSV {}

impl DataSourceLineConverter for DataSourceFormatTSV {
    fn get_cells(&self, line_set: &LineSet) -> Result<CellSet,GulpError> {
        DataSource::get_cells_xsv(b'\t', line_set)
    }
}

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

    pub fn line_converter(&self) -> Arc<Box<dyn DataSourceLineConverter>> {
        Arc::new(match self {
            Self::CSV => Box::new(DataSourceFormatCSV {}),
            Self::TSV => Box::new(DataSourceFormatTSV {}),
            Self::JSONL => Box::new(DataSourceFormatJSONL {}),
            Self::PAGEPILE => Box::new(DataSourceFormatPagePile {}),
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

    pub async fn get_cells(&self, limit: Option<usize>) -> Result<CellSet,GulpError> {
        let line_set = self.get_lines(limit).await?;
        let lh = self.source_format.line_converter();
        lh.get_cells(&line_set)
    }

    pub async fn guess_headers(&self, limit: Option<usize>, app: &Arc<AppState>) -> Result<Vec<HeaderColumn>,GulpError> {
        let line_set = self.get_lines(limit).await?;
        let lh = self.source_format.line_converter();
        let cell_set = lh.get_cells(&line_set)?;
        let mut new_headers = vec![];
        for (column,header) in cell_set.headers.iter().enumerate() {
            let cells = cell_set.get_cells_in_column(column);
            new_headers.push(header.guess(cells, app).await);
        }
        Ok(new_headers)
    }


    async fn get_lines(&self, limit: Option<usize>) -> Result<LineSet,GulpError> {
        let limit = limit.unwrap_or(usize::MAX);
        let lh = self.source_type.line_handler();
        lh.get_lines(&self, limit)
    }

    pub fn get_cells_xsv(separator: u8, line_set: &LineSet) -> Result<CellSet,GulpError> {
        let data = line_set.lines.join("\n");
        let data = data.as_bytes();
        let mut rdr = csv::ReaderBuilder::new()
            .has_headers(false)
            .delimiter(separator)
            .from_reader(data);
        let mut rows: Vec<_> = vec!();
        let mut headers = line_set.headers.to_owned();
        for result in rdr.records() {
            let record = result?;
            while headers.len()<record.len() {
                headers.push(HeaderColumn { column_type: ColumnType::String, wiki: None, string: None, namespace_id: None });
            }
            let cells: Vec<Option<Cell>> = record
                .iter()
                .zip(headers.iter())
                .map(|(value,column)|{
                    let value = serde_json::Value::String(value.to_string());
                    Cell::from_value(&value, column)
                })
                .collect();
            rows.push(cells);            
        }
        Ok(CellSet{headers,rows})
    }

}