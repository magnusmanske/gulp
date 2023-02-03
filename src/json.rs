use crate::row::Row;
use mysql_async::{prelude::*, Conn};
use crate::header::*;
// use crate::GenericError;

#[derive(Clone, Debug)]
pub struct JsonRow {
    pub id: DbId,
    pub json_text: String,
    pub md5: String,
}

impl JsonRow {
    pub async fn get_or_create(conn: &mut Conn, json_text: &str) -> Option<Self> {
        if let Some(ret) = Self::get_from_json(conn, &json_text).await {
            return Some(ret);
        }
        let sql = r#"INSERT IGNORE INTO `json` (json) VALUES (:json_text)"#;
        conn.exec_drop(sql, params!{json_text}).await.ok()?;
        let last_id = conn.last_insert_id().ok_or_else(||"JsonRow::get_or_create").ok()?;
        Some(Self {
            id: last_id,
            json_text: json_text.to_owned(),
            md5: Row::md5(json_text),
        })
    }

    pub async fn get_from_json(conn: &mut Conn, json_text: &str) -> Option<Self> {
        let sql = r#"SELECT json.id,json.json,json.md5 FROM json WHERE json.md5=MD5(:json_text) AND json.json=json_text"#;
        conn
            .exec_iter(sql,params! {json_text}).await.ok()?
            .map_and_drop(|row| Self::from_row(&row)).await.ok()?.get(0)?.to_owned()
    }

    pub async fn get_from_json_for_list(conn: &mut Conn, list_id: DbId, json_text: &str) -> Option<Self> {
        let sql = r#"SELECT json.id,json.json,json.md5 FROM `row`,json
            WHERE revision_id=(SELECT max(revision_id) FROM `row` i WHERE i.row_num = row.row_num AND i.list_id=:list_id)
            AND list_id=:list_id
            AND json.id=json_id
            AND json.md5=MD5(:json_text)
            AND json.json=json_text"#;
        conn
            .exec_iter(sql,params! {list_id,json_text}).await.ok()?
            .map_and_drop(|row| Self::from_row(&row)).await.ok()?.get(0)?.to_owned()
    }

    fn from_row(row: &mysql_async::Row) -> Option<Self> {
        Some(Self{
            id: row.get(0)?,
            json_text: row.get(1)?,
            md5: row.get(2)?,
        })
    }
}