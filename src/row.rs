use crate::cell::*;
use crate::header::*;
use mysql_async::{prelude::*, Conn};
use serde::{Deserialize, Serialize};
use serde_json::json;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Row {
    pub id: DbId,
    pub list_id: DbId,
    pub row_num: DbId,
    pub revision_id: DbId,
    pub json: String,
    pub json_md5: String,
    pub user_id: DbId,
    pub modified: String,
    pub cells: Vec<Option<Cell>>,
}

impl Row {
    pub async fn from_db(
        conn: &mut Conn,
        list_id: DbId,
        row_num: DbId,
        revision_id: DbId,
        header: &Header,
    ) -> Option<Self> {
        let sql = r#"SELECT row.id,list_id,row_num,revision_id,json,json_md5,user_id,modified
            FROM `row`
            WHERE list_id=:list_id AND row_num=:row_num AND revision_id<=:revision_id
            ORDER BY revision_id DESC LIMIT 1"#;
        conn.exec_iter(sql, params! {list_id,row_num,revision_id})
            .await
            .ok()?
            .map_and_drop(|row| Self::from_row(&row, header))
            .await
            .ok()?
            .first()?
            .to_owned()
    }

    pub fn new() -> Self {
        Self {
            id: 0,
            list_id: 0,
            row_num: 0,
            revision_id: 0,
            json: String::new(),
            json_md5: String::new(),
            user_id: 0,
            modified: String::new(),
            cells: vec![],
        }
    }

    pub fn from_cells(cells: Vec<Option<Cell>>) -> Self {
        let mut ret = Self::new();
        ret.cells = cells;
        ret
    }

    fn get_timestamp_from_row(v: &mysql_async::Value) -> String {
        v.as_sql(true).replace('\'', "")
    }

    pub fn from_row(row: &mysql_async::Row, header: &Header) -> Option<Self> {
        let json: String = row.get(4)?;
        let json: serde_json::Value = serde_json::from_str(&json).ok()?;
        let cells = json
            .as_array()?
            .iter()
            .zip(header.schema.columns.iter())
            .map(|(value, column)| Cell::from_value(value, column))
            .collect();
        Some(Self {
            id: row.get(0)?,
            list_id: row.get(1)?,
            row_num: row.get(2)?,
            revision_id: row.get(3)?,
            json: row.get(4)?,
            json_md5: row.get(5)?,
            user_id: row.get(6)?,
            modified: Self::get_timestamp_from_row(&row.get(7)?),
            cells,
        })
    }

    pub async fn row_exists_for_revision(
        conn: &mut Conn,
        list_id: DbId,
        revision_id: DbId,
        json_text: &str,
    ) -> Option<bool> {
        let sql = r#"SELECT id FROM `row`
            WHERE revision_id=(SELECT max(revision_id) FROM `row` i WHERE i.row_num = row.row_num AND i.list_id=:list_id AND revision_id<=:revision_id)
            AND list_id=:list_id
            AND json_md5=MD5(:json_text)
            AND json=:json_text"#;
        Some(
            !conn
                .exec_iter(sql, params! {list_id,json_text,revision_id})
                .await
                .ok()?
                .map_and_drop(|_row| 1)
                .await
                .ok()?
                .is_empty(),
        )
    }

    pub async fn add_or_replace(
        &mut self,
        header: &Header,
        conn: &mut Conn,
        user_id: DbId,
    ) -> Result<(), crate::GulpError> {
        let sql = r#"REPLACE INTO `row` (list_id,row_num,revision_id,json,json_md5,user_id) VALUES (:list_id,:row_num,:revision_id,:json,:json_md5,:user_id)"#;
        let list_id = self.list_id;
        let row_num = self.row_num;
        let revision_id = self.revision_id;

        let json = self.as_json(header)["c"].to_owned();
        let json = serde_json::to_string(&json)?;
        let json_md5 = Self::md5(&json);

        conn.exec_drop(
            sql,
            params! {list_id,row_num,revision_id,json,json_md5,user_id},
        )
        .await?;
        self.id = conn.last_insert_id().ok_or("Row::add_or_replace")?;
        Ok(())
    }

    pub fn md5(s: &str) -> String {
        format!("{:x}", md5::compute(s))
    }

    pub fn as_json(&self, header: &Header) -> serde_json::Value {
        let cells: Vec<serde_json::Value> = self
            .cells
            .iter()
            .zip(header.schema.columns.iter())
            .map(|(cell, column)| match cell {
                Some(c) => c.as_json(column),
                None => json!(null),
            })
            .collect();
        let ret = json!({
            "row": self.row_num,
            "modified": self.modified,
            "user": self.user_id,
            "c":cells,
        });
        json!(ret)
    }

    pub fn as_vec(&self, header: &Header) -> Vec<String> {
        let mut ret: Vec<String> = self
            .cells
            .iter()
            .zip(header.schema.columns.iter())
            .map(|(cell, column)| match cell {
                Some(c) => c.as_string(column),
                None => String::new(),
            })
            .collect();
        ret.insert(0, format!("{}", self.row_num));
        ret
    }

    pub fn as_tsv(&self, header: &Header) -> String {
        self.as_vec(header).join("\t")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app_state::*;

    #[test]
    fn test_md5() {
        assert_eq!(
            Row::md5("hello world"),
            "5eb63bbbe01eeed093cb22bb8f5acdc3".to_string()
        );
    }

    #[tokio::test]
    async fn test_from_db() {
        let app = AppState::from_config_file("config.json").expect("app creation failed");
        let mut conn = app.get_gulp_conn().await.expect("get_gulp_conn");
        let header = Header::from_list_id(&mut conn, 4)
            .await
            .expect("from_list_id");
        let row = Row::from_db(&mut conn, 4, 1, 1, &header)
            .await
            .expect("from_db");
        assert_eq!(row.id, 1);
        assert_eq!(
            row.json,
            "[\"Q111028176\",\"Buergerwehrbrunnen Bensheim.jpg\"]"
        );
        assert_eq!(row.cells.len(), 2);
    }
}
