use mysql_async::{prelude::*, Conn};
use serde::Serialize;

pub type NamespaceType = i64;
pub type DbId = u64;

#[derive(Clone, Debug, Serialize)]
pub enum ColumnType {
    String,
    WikiPage,
}

impl ColumnType {
    fn from_str(s: &str) -> Option<Self> {
        match s {
            "string" => Some(Self::String),
            "page" => Some(Self::WikiPage),
            _ => None
        }
    }
}



#[derive(Clone, Debug, Serialize)]
pub struct HeaderColumn {
    pub column_type: ColumnType,
    pub wiki: Option<String>,
    pub namespace_id: Option<NamespaceType>,
}

impl HeaderColumn {
    pub fn from_value(value: &serde_json::Value) -> Option<Self> {
        let o = value.as_object()?;
        Some(Self{
            column_type: ColumnType::from_str(o.get("type")?.as_str()?)?,
            wiki: o["wiki"].as_str().map(|s|s.to_string()),
            namespace_id: Self::value_option_to_namespace_id(o.get("namespace_id")),
        })
    }

    fn value_option_to_namespace_id(vo: Option<&serde_json::Value>) -> Option<NamespaceType> {
        vo?.as_i64()
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

    pub fn from_row(row: &mysql_async::Row) -> Option<Self> {
        let json: String = row.get(2)?;
        let json: serde_json::Value = serde_json::from_str(&json).ok()?;
        let mut columns : Vec<HeaderColumn> = vec![];
        for column in json.as_object()?.get("columns")?.as_array()? {
            columns.push(HeaderColumn::from_value(column)?);
        }
        Some(Self {
            id: row.get(0)?,
            name: row.get(1)?,
            columns,
        })
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
        let sql = r#"SELECT id,list_id,revision_id,header_schema_id FROM header WHERE list_id=:list_id AND revision_id<=:revision_id ORDER BY list_id DESC LIMIT 1"#;
        let result = conn
            .exec_iter(sql,params! {list_id,revision_id}).await.ok()?
            .map_and_drop(|row| mysql_async::from_row::<(DbId,DbId,DbId,DbId)>(row)).await.ok()?
            .get(0)?.to_owned();
        Some(Self {
            id: result.0,
            list_id: result.1, 
            revision_id: result.2, 
            schema: HeaderSchema::from_id(conn, result.3).await?,
        })
    }
}
