use crate::{column::ColumnType, header::*};
use serde::{Deserialize, Serialize};
use serde_json::json;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct WikiPage {
    pub title: String,
    pub namespace_id: Option<NamespaceType>,
    pub wiki: Option<String>,
}

impl WikiPage {
    pub fn as_json(&self, column: &HeaderColumn) -> serde_json::Value {
        if self.wiki == column.wiki && self.namespace_id == column.namespace_id {
            json!(self.title) // Short version, string only
        } else {
            json!(self) // Long version
        }
    }

    pub fn as_string(&self, column: &HeaderColumn) -> String {
        if self.wiki == column.wiki && self.namespace_id == column.namespace_id {
            self.title.to_owned()
        } else {
            format!("{:?}:{:?}:{}", &self.wiki, &self.namespace_id, &self.title)
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Location {
    lat: f64,
    lon: f64,
}

impl Location {
    pub fn as_json(&self, _column: &HeaderColumn) -> serde_json::Value {
        json!(self)
    }

    pub fn as_string(&self, _column: &HeaderColumn) -> String {
        format!("{}, {}", self.lat, self.lon)
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum Cell {
    WikiPage(WikiPage),
    String(String),
    Location(Location),
}

impl Cell {
    pub fn from_value(value: &serde_json::Value, column: &HeaderColumn) -> Option<Self> {
        match column.column_type {
            ColumnType::String => Some(Self::String(value.as_str()?.to_string())),
            ColumnType::WikiPage => Self::new_wiki_page(value, column),
            ColumnType::Location => Self::new_location(value),
        }
    }

    fn new_location(value: &serde_json::Value) -> Option<Self> {
        if let Some(s) = value.as_str() {
            let mut ret = None;
            for cap in crate::header::RE_LOCATION.captures_iter(s) {
                ret = Some(Self::Location(Location {
                    lat: cap[1].parse::<f64>().ok()?,
                    lon: cap[2].parse::<f64>().ok()?,
                }));
            }
            ret
        } else {
            Some(Self::Location(Location {
                lat: Self::value_as_f64(&value["lat"])?,
                lon: Self::value_as_f64(&value["lon"])?,
            }))
        }
    }

    // Gets a f64 from a Value, if it is either str or f64
    fn value_as_f64(v: &serde_json::Value) -> Option<f64> {
        if let Some(s) = v.as_str() {
            s.parse::<f64>().ok()
        } else {
            v.as_f64()
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
            let wiki = match o.get("wiki") {
                Some(wiki) => match wiki.as_str() {
                    Some(wiki) => Some(wiki.to_string()),
                    None => column.wiki.to_owned(),
                },
                None => column.wiki.to_owned(),
            };
            let namespace_id = match o.get("namespace_id") {
                Some(id) => match id.as_i64() {
                    Some(id) => Some(id),
                    None => column.namespace_id.to_owned(),
                },
                None => column.namespace_id.to_owned(),
            };
            WikiPage {
                title: o.get("title")?.as_str()?.to_string(),
                namespace_id,
                wiki,
            }
        } else {
            //dbg!(format!("new_wiki_page: {value:?}"));
            return None;
        };
        Some(Self::WikiPage(page))
    }

    pub fn as_json(&self, column: &HeaderColumn) -> serde_json::Value {
        match self {
            Cell::String(s) => json!(s),
            Cell::WikiPage(wp) => wp.as_json(column),
            Cell::Location(location) => location.as_json(column),
        }
    }

    pub fn as_string(&self, column: &HeaderColumn) -> String {
        match self {
            Cell::String(s) => s.to_owned(),
            Cell::WikiPage(wp) => wp.as_string(column),
            Cell::Location(location) => location.as_string(column),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_wiki_page() {
        let column = HeaderColumn {
            column_type: ColumnType::WikiPage,
            wiki: None,
            string: None,
            namespace_id: None,
        };
        let j = json!({"title":"Abc","namespace_id":7,"wiki":"frwiki"});
        let c = Cell::new_wiki_page(&j, &column).expect("new_wiki_page failed");
        let wp = match c {
            Cell::WikiPage(wp) => wp,
            _ => panic!("Not a WikiPage"),
        };
        assert_eq!(wp.title, "Abc");
        assert_eq!(wp.namespace_id, Some(7));
        assert_eq!(wp.wiki, Some("frwiki".to_string()));
        assert_eq!(wp.as_json(&column), j); // Round trip
    }
}
