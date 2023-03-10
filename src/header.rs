use futures::future::join_all;
use mysql_async::{prelude::*, Conn};
use regex::Regex;
use std::{collections::HashMap, sync::Arc};
use serde::Serialize;
use serde_json::json;

use crate::{app_state::AppState, cell::Cell};

lazy_static!{
    static ref RE_WIKIDATA : Regex = Regex::new(r#"^[PQ]\d+$"#).expect("Regexp error");
    static ref RE_WIKIDATA_ITEM : Regex = Regex::new(r#"^Q\d+$"#).expect("Regexp error");
    static ref RE_FILE : Regex = Regex::new(r#"^(?i).+\.(jpg|jpeg|tif|tiff|png)$"#).expect("Regexp error");
}


pub type NamespaceType = i64;
pub type DbId = u64;

#[derive(Clone, Debug, PartialEq, Serialize)]
pub enum ColumnType {
    String,
    WikiPage,
}

impl ColumnType {
    fn from_str(s: &str) -> Option<Self> {
        match s {
            "String" => Some(Self::String),
            "WikiPage" => Some(Self::WikiPage),
            _ => None
        }
    }
}



#[derive(Clone, Debug, Serialize)]
pub struct HeaderColumn {
    pub column_type: ColumnType,
    pub wiki: Option<String>,
    pub string: Option<String>,
    pub namespace_id: Option<NamespaceType>,
}

impl HeaderColumn {
    pub fn from_value(value: &serde_json::Value) -> Option<Self> {
        let ct = value.get("column_type")?.as_str()?;
        Some(Self{
            column_type: ColumnType::from_str(ct)?,
            wiki: Self::value_option_to_string_option(value.get("wiki")),
            namespace_id: Self::value_option_to_namespace_id(value.get("namespace_id")),
            string: Self::value_option_to_string_option(value.get("string")),
        })
    }

    pub async fn guess(&self, cells: Vec<Cell>, _app: &Arc<AppState>) -> HeaderColumn {
        if self.column_type!=ColumnType::String || self.wiki.is_some() || self.string.is_some() || self.namespace_id.is_some() {
            return self.to_owned();
        }
        let mut pages_to_check = vec![];
        let mut files_to_check = vec![];
        let mut stats = HashMap::from([
            ("total",0),
            ("wikidata",0),
            ("wikidata_ns0",0),
            ("file",0),
            ("commons_ns6",0),
            ]);
        for cell in &cells {
            *stats.get_mut("total").unwrap() += 1 ;
            match cell {
                Cell::WikiPage(_) => todo!(),
                Cell::String(s) => {
                    *stats.get_mut("wikidata").unwrap() += RE_WIKIDATA.is_match(&s) as u32;
                    *stats.get_mut("wikidata_ns0").unwrap() += RE_WIKIDATA_ITEM.is_match(&s) as u32;
                    if RE_FILE.is_match(&s) {
                        *stats.get_mut("file").unwrap() += 1;
                        files_to_check.push(format!("File:{s}"));
                    }
                    pages_to_check.push(s.replace("_"," "));
                },
            }
        }
        if !pages_to_check.is_empty() {
            // todo!()
        }
        if !files_to_check.is_empty() {
            // `urls` needs to outlive `futures`
            let urls: Vec<_> = files_to_check
                .chunks(50)
                .map(|chunk|format!("https://commons.wikimedia.org/w/api.php?action=query&format=json&prop=info&titles={}",chunk.join("|")))
                .collect();
            let futures: Vec<_> = urls.iter().map(|url|AppState::get_url_as_json(url)).collect();
            let files_found: usize = join_all(futures)
                .await
                .iter()
                .cloned()
                .filter_map(|r|r)
                .filter_map(|r|r.get("query").map(|r|r.to_owned()))
                .filter_map(|r|r.get("pages").map(|r|r.to_owned()))
                .filter_map(|r|r.as_object().map(|r|r.to_owned()))
                .map(|o|o.values().cloned().collect::<Vec<serde_json::Value>>())
                .flatten()
                .filter(|v|v.get("missing").is_none())
                .count();
            *stats.get_mut("commons_ns6").unwrap() += files_found as u32;
        }
        if stats["wikidata"]==stats["total"] {
            let ret = HeaderColumn {
                column_type: ColumnType::WikiPage, 
                wiki: Some("wikidatawiki".into()),
                string: None, 
                namespace_id: if stats["wikidata_ns0"]==stats["total"] { Some(0) } else { None },
            };
            return ret;
        }
        if stats["commons_ns6"]>=stats["total"]*9/10 { // 90% are Commons files, good enough
            let ret = HeaderColumn {
                column_type: ColumnType::WikiPage, 
                wiki: Some("commonswiki".into()),
                string: None, 
                namespace_id: Some(6),
            };
            return ret;
        }
        return self.to_owned();
    }

    fn value_option_to_namespace_id(vo: Option<&serde_json::Value>) -> Option<NamespaceType> {
        vo?.as_i64()
    }

    fn value_option_to_string_option(vo: Option<&serde_json::Value>) -> Option<String> {
        Some(vo.to_owned()?.as_str()?.to_string())
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct HeaderSchema {
    pub id: DbId,
    pub name: String,
    pub columns: Vec<HeaderColumn>,
}

impl HeaderSchema {
    pub async fn from_id(conn: &mut Conn, header_schema_id: DbId) -> Option<Self> {
        let sql = r#"SELECT header_schema.id,name,json FROM header_schema WHERE header_schema.id=:header_schema_id"#;
        conn
            .exec_iter(sql,params! {header_schema_id}).await.ok()?
            .map_and_drop(|row| Self::from_row(&row)).await.ok()?
            .get(0)?.to_owned()
    }

    pub fn from_name_json(name: &str, json: &str) -> Option<Self> {
        let json: serde_json::Value = serde_json::from_str(json).ok()?;
        let mut columns : Vec<HeaderColumn> = vec![];
        for column in json.get("columns")?.as_array()? {
            columns.push(HeaderColumn::from_value(column)?);
        }
        Some(Self {
            id: 0,
            name: name.to_string(),
            columns,
        })
    }

    pub fn from_row(row: &mysql_async::Row) -> Option<Self> {
        let json: String = row.get(2)?;
        let json: serde_json::Value = serde_json::from_str(&json).ok()?;
        let mut columns : Vec<HeaderColumn> = vec![];
        for column in json.get("columns")?.as_array()? {
            columns.push(HeaderColumn::from_value(column)?);
        }
        Some(Self {
            id: row.get(0)?,
            name: row.get(1)?,
            columns,
        })
    }

    pub async fn create_in_db(&mut self, app: &std::sync::Arc<AppState>) -> Result<DbId,crate::GulpError> {
        if self.id!=0 {
            return Err("create_in_db: Already has an id".into());
        }
        let mut conn = app.get_gulp_conn().await?;

        // Check if there is already a header schema with that exact JSON
        let name = self.name.to_string();
        let json = json!({"columns":self.columns}).to_string();
        let sql = "SELECT id,name,json FROM `header_schema` WHERE `json`=:json" ;
        if let Some(hs) = conn.exec_iter(sql,params! {json}).await?.map_and_drop(|row| Self::from_row(&row)).await?.get(0) {
            let hs = match hs.to_owned() {
                Some(hs) => hs,
                None => return Err(format!("create_in_db: result error"))?,
            };
            return Err(format!("create_in_db: A header schema with this JSON already exist: #{}: {}",hs.id,hs.name).into());
        }

        // Create new row
        let json = json!({"columns":self.columns}).to_string();
        let sql = r#"INSERT INTO `header_schema` (`name`,`json`) VALUES (:name,:json)"# ;
        conn.exec_drop(sql, params!{name,json}).await?;
        if let Some(id) = conn.last_insert_id() {
            self.id = id
        }
        Ok(self.id)
    }

    pub fn get_first_wiki_page_column(&self) -> Option<usize> {
        self.columns
        .iter()
        .enumerate()
        .filter_map(|(num,col)|{
            match col.column_type {
                ColumnType::WikiPage => Some(num),
                _ => None,
            }
        })
        .next()
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct Header {
    pub id: DbId,
    pub list_id: DbId,
    pub revision_id: DbId,
    pub schema: HeaderSchema,
}

impl Header {
    pub async fn from_list_id(conn: &mut Conn, list_id: DbId) -> Option<Self> {
        Self::from_list_revision_id(conn, list_id, DbId::MAX).await
    }

    pub async fn from_list_revision_id(conn: &mut Conn, list_id: DbId, revision_id: DbId) -> Option<Self> {
        let sql = r#"SELECT id,list_id,revision_id,header_schema_id FROM header WHERE list_id=:list_id AND revision_id<=:revision_id ORDER BY revision_id DESC LIMIT 1"#;
        let result = conn
            .exec_iter(sql,params! {list_id,revision_id}).await.ok()?
            .map_and_drop(|row| mysql_async::from_row::<(DbId,DbId,DbId,DbId)>(row)).await.ok()?
            .get(0)?.to_owned();
        let hs = HeaderSchema::from_id(conn, result.3).await;
        Some(Self {
            id: result.0,
            list_id: result.1, 
            revision_id: result.2, 
            schema: hs?,
        })
    }

    pub async fn create_in_db(&mut self, app: &std::sync::Arc<AppState>) -> Result<DbId,crate::GulpError> {
        if self.id!=0 {
            return Err("create_in_db: Already has an id".into());
        }
        let mut conn = app.get_gulp_conn().await?;

        let list_id = self.list_id;
        let revision_id = self.revision_id;
        let header_schema_id = self.schema.id;
        let sql = r#"INSERT INTO `header` (`list_id`,`revision_id`,`header_schema_id`) VALUES (:list_id,:revision_id,:header_schema_id)"# ;
        //println!("{sql}\n{list_id}/{revision_id}/{header_schema_id}");
        conn.exec_drop(sql, params!{list_id,revision_id,header_schema_id}).await?;
        if let Some(id) = conn.last_insert_id() {
            self.id = id
        }
        Ok(self.id)
    }}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_from_name_json() {
        let json_string = r#"{"columns":[{"column_type":"WikiPage"}]}"#;
        let hs = HeaderSchema::from_name_json("Test",&json_string).expect("from_name_json error");
        assert_eq!(hs.name,"Test");
        assert_eq!(hs.columns.len(),1);
        assert_eq!(hs.columns[0].column_type,ColumnType::WikiPage);
    }
}