use std::io::{self, BufRead, BufReader, Seek};
use std::sync::Arc;
use crate::app_state::AppState;
use crate::data_source::*;
use crate::row::Row;
use crate::{GulpError, header::*};
use crate::cell::*;

// type LinesReturnType = Box<dyn Iterator<Item = String>>;
type LinesReturnType = Vec<String>;

/// Trait for converting LineSet via various formats into CellSet
pub trait DataSourceLineConverter {
    fn get_cells(&self, line_set: &mut LineSet, limit: Option<usize>) -> Result<CellSet,GulpError> ;

    fn get_lines_old(&self, line_set: &mut LineSet, limit: usize) -> Result<LinesReturnType,GulpError> {
        let file = Arc::get_mut(&mut line_set.file).unwrap();
        let iterator = io::BufReader::new(file)
            .lines()
            .filter_map(|row|row.ok())
            .map(|s|s.trim().to_string())
            .take(limit)
            ;
        Ok(iterator.collect())
    }

    fn get_lines(&self, line_set: &mut LineSet, limit: usize) -> Result<LinesReturnType,GulpError> {
        let file = Arc::get_mut(&mut line_set.file).unwrap();
        let mut reader = io::BufReader::new(file);
        let mut lines = vec![];
        loop {
            let mut buffer_string = String::new();
            match reader.read_line(&mut buffer_string)? {
                0 => break,
                _ => lines.push(buffer_string),
            }
            if lines.len()>=limit {
                break;
            }
        }
        // println!("BEFORE: {:?}",reader.stream_position());
        reader.rewind()?;
        // println!("AFTER: {:?}",reader.stream_position());
        Ok(lines)
    }

    fn get_cells_xsv(&self, separator: u8, line_set: &mut LineSet, limit: Option<usize>) -> Result<CellSet,GulpError> {
        let limit = limit.unwrap_or(usize::MAX);
        let file = Arc::get_mut(&mut line_set.file).unwrap();
        let reader = BufReader::new(file);
        let mut rdr = csv::ReaderBuilder::new()
            .has_headers(false)
            .delimiter(separator)
            .from_reader(reader);
        let mut rows: Vec<_> = vec!();
        let mut headers = line_set.headers.to_owned();
        for result in rdr.records() {
            let record = result?;
            while headers.len()<record.len() {
                headers.push(HeaderColumn { column_type: ColumnType::String, wiki: None, string: None, namespace_id: None });
            }
            let mut row = Row::new();
            row.cells = record
                .iter()
                .zip(headers.iter())
                .map(|(value,column)|{
                    let value = serde_json::Value::String(value.to_string());
                    Cell::from_value(&value, column)
                })
                .collect();
            rows.push(row);
            if rows.len() >= limit {
                break;
            }
        }
        Ok(CellSet{headers,rows})
    }
}


impl DataSourceLineConverter for DataSourceFormatJSONL {
    fn get_cells(&self, line_set: &mut LineSet, limit: Option<usize>) -> Result<CellSet,GulpError> {
        let limit = limit.unwrap_or(usize::MAX);
        let mut headers = line_set.headers.to_owned();
        let lines = self.get_lines(line_set, limit)?;
        if headers.is_empty() {
            headers = self.guess_headers(&lines);
        }
        let mut rows: Vec<_> = vec!();
        for line in &lines {
            let json: serde_json::Value = serde_json::from_str(line)?;
            let array = json.as_array().ok_or_else(||"import_jsonl: valid JSON but not an array: {row}")?;
            let mut row = Row::new();
            row.cells = array
                .iter()
                .zip(headers.iter())
                .map(|(value,column)|Cell::from_value(value, column))
                .collect();
            rows.push(row);
        }
        Ok(CellSet{headers:headers.to_owned(),rows})
    }
}


impl DataSourceLineConverter for DataSourceFormatCSV {
    fn get_cells(&self, line_set: &mut LineSet, limit: Option<usize>) -> Result<CellSet,GulpError> {
        self.get_cells_xsv(b',', line_set, limit)
    }
}


impl DataSourceLineConverter for DataSourceFormatTSV {
    fn get_cells(&self, line_set: &mut LineSet, limit: Option<usize>) -> Result<CellSet,GulpError> {
        self.get_cells_xsv(b'\t', line_set, limit)
    }
}

impl DataSourceLineConverter for DataSourceFormatPagePile {
    fn get_cells(&self, line_set: &mut LineSet, limit: Option<usize>) -> Result<CellSet,GulpError> {
        let limit = limit.unwrap_or(usize::MAX);
        let (wiki,lines) = self.get_lines_actually(line_set, limit)?;
        let headers = vec![HeaderColumn{ column_type: ColumnType::WikiPage, wiki:Some(wiki), string: None, namespace_id: None }];
        let wiki = headers[0].wiki.as_ref().ok_or_else(||"PagePile header has no wiki")?;
        let api = AppState::get_api_for_wiki(wiki)?;
        let rows: Vec<Row> = lines
            .iter()
            .map(|line|wikibase::mediawiki::title::Title::new_from_full(&line, &api))
            .map(|title|WikiPage{ title: title.pretty().to_string(), namespace_id: Some(title.namespace_id()), wiki: Some(wiki.to_owned()) })
            .map(|page|vec![Some(Cell::WikiPage(page))])
            .map(|cells|Row::from_cells(cells))
            .collect();
        Ok(CellSet{headers,rows})
    }
}
