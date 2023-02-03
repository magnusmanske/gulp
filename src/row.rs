use mysql_async::{prelude::*, Conn};
use crate::header::*;
use crate::cell::*;
// use crate::GenericError;

#[derive(Clone, Debug)]
pub struct Row {
    pub id: DbId,
    pub list_id: DbId,
    pub row_num: DbId,
    pub revision_id: DbId,
    pub json: String,
    pub json_md5: String,
    pub cells: Vec<Option<Cell>>,
}

impl Row {
    pub async fn from_db(conn: &mut Conn, list_id: DbId, row_num: DbId, revision_id: DbId, header: &Header) -> Option<Self> {
        let sql = r#"SELECT row.id,list_id,row_num,revision_id,json,json_md5
            FROM `row`
            WHERE list_id=:list_id AND row_num=:row_num AND revision_id<=:revision_id
            ORDER BY revision_id DESC LIMIT 1"#;
        conn
            .exec_iter(sql,params! {list_id,row_num,revision_id}).await.ok()?
            .map_and_drop(|row| Self::from_row(&row,header)).await.ok()?
            .get(0)?.to_owned()
    }

    pub fn from_row(row: &mysql_async::Row, header: &Header) -> Option<Self> {
        let json: String = row.get(5)?;
        let json: serde_json::Value = serde_json::from_str(&json).ok()?;
        let cells = json
            .as_array()?
            .iter()
            .zip(header.schema.columns.iter())
            .map(|(value,column)|Cell::from_value(value, column))
            .collect();
        Some(Self {
            id: row.get(0)?,
            list_id: row.get(1)?,
            row_num: row.get(2)?,
            revision_id: row.get(3)?,
            json: row.get(4)?,
            json_md5: row.get(5)?,
            cells,
        })
    }

    pub async fn row_exists_for_revision(conn: &mut Conn, list_id: DbId, revision_id: DbId, json_text: &str) -> Option<bool> {
        let sql = r#"SELECT id FROM `row`
            WHERE revision_id=(SELECT max(revision_id) FROM `row` i WHERE i.row_num = row.row_num AND i.list_id=:list_id AND revision_id<=:revision_id)
            AND list_id=:list_id
            AND json_md5=MD5(:json_text)
            AND json=:json_text"#;
        Some(!conn
            .exec_iter(sql,params! {list_id,json_text,revision_id}).await.ok()?
            .map_and_drop(|_row| 1).await.ok()?.is_empty())
    }
/*
    pub async fn insert_new(&mut self, conn: &mut conn) -> Result<(), GenericError> {
        let sql = r#"REPLACE INTO `row` (list_id,row_num,revision_id,json_id) VALUES (:list_id,:row_num,:revision_id,:json)"#;
        let list_id = self.list_id;
        let row_num = self.row_num;
        let revision_id = self.revision_id;
        let json = &self.json;
        conn.exec_drop(sql, params!{list_id,row_num,revision_id,json}).await?;
        self.id = conn.last_insert_id().ok_or_else(||"Row::insert_new")?;
        Ok(())
    } */

    pub fn md5(s: &str) -> String {
        format!("{:x}",md5::compute(s))
    }

}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_md5() {
        assert_eq!(Row::md5("hello world"),"5eb63bbbe01eeed093cb22bb8f5acdc3".to_string());
    }
}
