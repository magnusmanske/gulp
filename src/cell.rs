use serde_json::json;
use serde::{Deserialize, Serialize};
use crate::header::*;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct WikiPage {
    pub title: String,
    pub namespace_id: Option<NamespaceType>,
    pub wiki: Option<String>,
}

impl WikiPage {
    pub fn as_json(&self, column: &HeaderColumn) -> serde_json::Value {
        if self.wiki==column.wiki && self.namespace_id==column.namespace_id {
            json!(self.title) // Short version, string only
        } else {
            json!(self) // Long version
        }
    }

    pub fn as_string(&self, column: &HeaderColumn) -> String {
        if self.wiki==column.wiki && self.namespace_id==column.namespace_id {
            self.title.to_owned()
        } else {
            format!("{:?}:{:?}:{}",&self.wiki,&self.namespace_id,&self.title)
        }
    }
}



#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum Cell {
    WikiPage(WikiPage),
    String(String),
}

impl Cell {
    pub fn from_value(value: &serde_json::Value, column: &HeaderColumn) -> Option<Self> {
        match column.column_type {
            ColumnType::String => Some(Self::String(value.as_str()?.to_string())),
            ColumnType::WikiPage => Some(Self::new_wiki_page(value, column)?),
        }
    }

    fn new_wiki_page(value: &serde_json::Value, column: &HeaderColumn) -> Option<Self> {
        let page = if let Some(s) = value.as_str() {
            WikiPage {
                title: s.to_string(),
                namespace_id: column.namespace_id.to_owned(),
                wiki: column.wiki.to_owned(),
            }
        } else if let Some(o) = value.as_object() {
            WikiPage {
                title: o.get("title")?.as_str()?.to_string(),
                namespace_id: column.namespace_id.to_owned(),
                wiki: column.wiki.to_owned(),
            }
        } else {
            dbg!(format!("new_wiki_page: {value:?}"));
            return None;
        };
        Some(Self::WikiPage(page))
    }

    pub fn as_json(&self, column: &HeaderColumn) -> serde_json::Value {
        match self {
            Cell::String(s) => json!(s),
            Cell::WikiPage(wp) => wp.as_json(column),
        }
    }

    pub fn as_string(&self, column: &HeaderColumn) -> String {
        match self {
            Cell::String(s) => s.to_owned(),
            Cell::WikiPage(wp) => wp.as_string(column),
        }
    }
}