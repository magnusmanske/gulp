use std::io::{self, BufRead, BufReader, Seek};
use std::sync::Arc;
use tempfile::tempdir;

use crate::app_state::AppState;
use crate::cell::*;
use crate::column::ColumnType;
use crate::data_source::*;
use crate::row::Row;
use crate::{header::*, GulpError};

type LinesReturnType = Vec<String>;

/// Trait for converting FileWithHeader via various formats into CellSet
pub trait DataSourceLineConverter {
    fn get_cells(
        &self,
        header_file: &mut FileWithHeader,
        limit: Option<usize>,
    ) -> Result<CellSet, GulpError>;

    fn get_lines(
        &self,
        header_file: &mut FileWithHeader,
        limit: usize,
    ) -> Result<LinesReturnType, GulpError> {
        let file = Arc::get_mut(&mut header_file.file).unwrap();
        let mut reader = io::BufReader::new(file);
        let mut lines = vec![];
        loop {
            let mut buffer_string = String::new();
            match reader.read_line(&mut buffer_string)? {
                0 => break,
                _ => lines.push(buffer_string),
            }
            if lines.len() >= limit {
                break;
            }
        }
        reader.rewind()?;
        Ok(lines)
    }

    fn get_cells_xsv(
        &self,
        separator: u8,
        header_file: &mut FileWithHeader,
        limit: Option<usize>,
    ) -> Result<CellSet, GulpError> {
        let limit = limit.unwrap_or(usize::MAX);
        let file = Arc::get_mut(&mut header_file.file).unwrap();
        let reader = BufReader::new(file);
        let mut rdr = csv::ReaderBuilder::new()
            .has_headers(false)
            .delimiter(separator)
            .from_reader(reader);
        let mut rows: Vec<_> = vec![];
        let mut headers = header_file.headers.to_owned();
        for result in rdr.records() {
            let record = result?;
            while headers.len() < record.len() {
                headers.push(HeaderColumn {
                    column_type: ColumnType::String,
                    wiki: None,
                    string: None,
                    namespace_id: None,
                });
            }
            let mut row = Row::new();
            row.cells = record
                .iter()
                .zip(headers.iter())
                .map(|(value, column)| {
                    let value = serde_json::Value::String(value.to_string());
                    Cell::from_value(&value, column)
                })
                .collect();
            rows.push(row);
            if rows.len() >= limit {
                break;
            }
        }
        Ok(CellSet { headers, rows })
    }
}

impl DataSourceLineConverter for DataSourceFormatJSONL {
    fn get_cells(
        &self,
        header_file: &mut FileWithHeader,
        limit: Option<usize>,
    ) -> Result<CellSet, GulpError> {
        let limit = limit.unwrap_or(usize::MAX);
        let mut headers = header_file.headers.to_owned();
        let lines = self.get_lines(header_file, limit)?;
        if headers.is_empty() {
            headers = self.guess_headers(&lines);
        }
        let mut rows: Vec<_> = vec![];
        for line in &lines {
            let json: serde_json::Value = serde_json::from_str(line)?;
            let array = json
                .as_array()
                .ok_or("import_jsonl: valid JSON but not an array: {row}")?;
            let mut row = Row::new();
            row.cells = array
                .iter()
                .zip(headers.iter())
                .map(|(value, column)| Cell::from_value(value, column))
                .collect();
            rows.push(row);
        }
        Ok(CellSet {
            headers: headers.to_owned(),
            rows,
        })
    }
}

impl DataSourceLineConverter for DataSourceFormatCSV {
    fn get_cells(
        &self,
        header_file: &mut FileWithHeader,
        limit: Option<usize>,
    ) -> Result<CellSet, GulpError> {
        self.get_cells_xsv(b',', header_file, limit)
    }
}

impl DataSourceLineConverter for DataSourceFormatTSV {
    fn get_cells(
        &self,
        header_file: &mut FileWithHeader,
        limit: Option<usize>,
    ) -> Result<CellSet, GulpError> {
        self.get_cells_xsv(b'\t', header_file, limit)
    }
}

impl DataSourceLineConverter for DataSourceFormatPagePile {
    fn get_cells(
        &self,
        header_file: &mut FileWithHeader,
        limit: Option<usize>,
    ) -> Result<CellSet, GulpError> {
        let limit = limit.unwrap_or(usize::MAX);
        let (wiki, lines) = self.get_lines_actually(header_file, limit)?;
        let headers = vec![HeaderColumn {
            column_type: ColumnType::WikiPage,
            wiki: Some(wiki),
            string: None,
            namespace_id: None,
        }];
        let wiki = headers[0]
            .wiki
            .as_ref()
            .ok_or("PagePile header has no wiki")?;
        let api = AppState::get_api_for_wiki(wiki)?;
        let rows: Vec<Row> = lines
            .iter()
            .map(|line| wikibase::mediawiki::title::Title::new_from_full(line, &api))
            .map(|title| WikiPage {
                title: title.pretty().to_string(),
                namespace_id: Some(title.namespace_id()),
                wiki: Some(wiki.to_owned()),
            })
            .map(|page| vec![Some(Cell::WikiPage(page))])
            .map(Row::from_cells)
            .collect();
        Ok(CellSet { headers, rows })
    }
}

impl DataSourceLineConverter for DataSourceFormatExcel {
    fn get_cells(
        &self,
        header_file: &mut FileWithHeader,
        limit: Option<usize>,
    ) -> Result<CellSet, GulpError> {
        let limit = limit.unwrap_or(usize::MAX);
        let mut reader =
            Arc::get_mut(&mut header_file.file).ok_or("Cannot get temporaryfile handle")?;

        let tmpdir = tempdir()?;
        let file_path = tmpdir.path().join("temporary.xlsx");
        let mut writer = std::fs::File::create(file_path.clone())?;
        io::copy(&mut reader, &mut writer)?;
        drop(writer);
        reader.rewind()?;

        let mut workbook = office::Excel::open(file_path).map_err(|e| e.to_string())?;
        let first_workbook_name = workbook
            .sheet_names()
            .map_err(|e| e.to_string())?
            .first()
            .ok_or("No worksheets in Excel file")?
            .to_owned();
        let range = workbook
            .worksheet_range(&first_workbook_name)
            .map_err(|e| e.to_string())?;

        let mut rows: Vec<_> = vec![];
        let mut headers = header_file.headers.to_owned();
        for record in range.rows() {
            while headers.len() < record.len() {
                headers.push(HeaderColumn {
                    column_type: ColumnType::String,
                    wiki: None,
                    string: None,
                    namespace_id: None,
                });
            }
            let mut row = Row::new();
            row.cells = record
                .iter()
                .zip(headers.iter())
                .map(|(value, column)| {
                    let value: String = match value {
                        office::DataType::Int(i) => format!("{i}"),
                        office::DataType::Float(f) => format!("{f}"),
                        office::DataType::String(s) => s.to_owned(),
                        office::DataType::Bool(b) => format!("{}", *b as u8),
                        office::DataType::Error(_) => "".into(),
                        office::DataType::Empty => "".into(),
                    };
                    let value = serde_json::Value::String(value);
                    Cell::from_value(&value, column)
                })
                .collect();
            rows.push(row);
            if rows.len() >= limit {
                break;
            }
        }
        tmpdir.close()?; // Cleanup
        Ok(CellSet { headers, rows })
    }
}
