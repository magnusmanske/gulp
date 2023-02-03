use std::collections::HashSet;
use serde_json::json;
use mysql_async::{prelude::*, Conn};
use crate::header::*;
use crate::cell::*;
use crate::row::*;
use crate::GenericError;

const ROW_INSERT_BATCH_SIZE: usize = 1000;

#[derive(Clone, Debug)]
pub enum FileType {
    TSV,
    CSV,
    JSONL,
}

#[derive(Clone, Debug)]
pub struct List {
    pub id: DbId,
    pub name: String,
    pub revision_id: DbId,
    pub header: Header,
}

impl List {
    pub async fn from_id(conn: &mut Conn, list_id: DbId) -> Option<Self> {
        let sql = r#"SELECT id,name,revision_id FROM `list` WHERE id=:list_id"#;
        let row = conn
            .exec_iter(sql,params! {list_id}).await.ok()?
            .map_and_drop(|row| row).await.ok()?
            .get(0)?.to_owned();
        Self::from_row(conn, &row, list_id).await
    }

    async fn from_row(conn: &mut Conn, row: &mysql_async::Row, list_id: DbId) -> Option<Self> {
        Some(Self {
            id: row.get(0)?,
            name: row.get(1)?,
            revision_id: row.get(2)?,
            header: Header::from_list_id(conn, list_id).await?,
        })
    }

    pub async fn import_from_url(&self, conn: &mut Conn, url: &str, file_type: FileType) -> Result<(), GenericError> {
        let client = reqwest::Client::builder()
            .user_agent("gulp/0.1")
            .build()?;
        let text = client.get(url).send().await?.text().await?;
        let _ = match file_type {
            FileType::JSONL => self.import_jsonl(conn,&text).await?,
            _ => return Err("import_from_url: unsopported type {file_type}".into()),
        };
        Ok(())
    }

    async fn load_json_md5s(&self, conn: &mut Conn) -> Result<HashSet<String>,GenericError> {
        let list_id = self.id;
        let sql = r#"SELECT json_md5 FROM `row`
            WHERE revision_id=(SELECT max(revision_id) FROM `row` i WHERE i.row_num = row.row_num AND i.list_id=:list_id)
            AND list_id=:list_id"#;
        let ret = conn
            .exec_iter(sql,params! {list_id}).await?
            .map_and_drop(|row| mysql_async::from_row::<String>(row)).await?;
        let ret: HashSet<String> = ret.into_iter().collect();
        Ok(ret)
    }

    async fn import_jsonl(&self, conn: &mut Conn, text: &str) -> Result<(), GenericError> {
        let mut md5s = self.load_json_md5s(conn).await?;
        let mut next_row_num = self.get_max_row_num(conn).await? + 1;
        println!("{} / {next_row_num}",md5s.len());
        let mut rows = vec![];
        for row in text.split("\n") {
            if row.is_empty() {
                continue;
            }
            let json: serde_json::Value = serde_json::from_str(row)?;
            let array = json.as_array().ok_or_else(||"import_jsonl: valid JSON but not an array: {row}".to_string())?;
            let cells: Vec<Option<Cell>> = array
                .iter()
                .zip(self.header.schema.columns.iter())
                .map(|(value,column)|Cell::from_value(value, column))
                .collect();
            if let Some(row) = self.get_or_ignore_new_row(conn, &md5s, cells, next_row_num).await? {
                next_row_num += 1;
                md5s.insert(row.json_md5.to_owned());
                rows.push(row);
                if rows.len()>=ROW_INSERT_BATCH_SIZE {
                    self.flush_row_insert(conn, &mut rows).await?;
                }
            }
        }
        self.flush_row_insert(conn, &mut rows).await?;
        Ok(())
    }

    async fn flush_row_insert(&self, conn: &mut Conn, rows: &mut Vec<Row>) -> Result<(), GenericError> {
        if rows.is_empty() {
            return Ok(());
        }
        let params: Vec<mysql_async::Params> = rows.iter().map(|row| {
            let list_id = row.list_id;
            let row_num = row.row_num;
            let revision_id = row.revision_id;
            let json = &row.json;
            let json_md5 = &row.json_md5;
            params!{list_id,row_num,revision_id,json,json_md5}
        }).collect();
        let sql = r#"INSERT INTO `row` (list_id,row_num,revision_id,json,json_md5) VALUES (:list_id,:row_num,:revision_id,:json,:json_md5)"#;
        let tx_opts = mysql_async::TxOpts::default()
            .with_consistent_snapshot(true)
            .with_isolation_level(mysql_async::IsolationLevel::RepeatableRead)
            .to_owned();
        let mut transaction = conn.start_transaction(tx_opts).await?;
        transaction.exec_batch(sql, params.iter()).await?;
        transaction.commit().await?;
        rows.clear();
        Ok(())
    }

    async fn check_json_exists(&self, _conn: &mut Conn, _json_text: &str, _json_md5: &str) -> Result<bool, GenericError> {
        // Already checked via md5, might have to implement if collisions occur
        /*
            let list_id = self.id;
            println!("Checking {json_md5}");
            let sql = "SELECT id FROM `row`
                WHERE revision_id=(SELECT max(revision_id) FROM `row` i WHERE i.row_num = row.row_num AND i.list_id=:list_id)
                AND list_id=:list_id
                AND json_md5=:json_md5
                AND json=:json_text";
            !conn
                .exec_iter(sql,params! {list_id,json_text,json_md5}).await?
                .map_and_drop(|_row| 1).await?.is_empty()
         */
        Ok(true)
    }

    async fn get_or_ignore_new_row(&self, conn: &mut Conn, md5s: &HashSet<String>, cells: Vec<Option<Cell>>, row_num: DbId) -> Result<Option<Row>, GenericError> {
        let cells2j: Vec<serde_json::Value> = cells
            .iter()
            .zip(self.header.schema.columns.iter())
            .map(|(cell,column)| cell.to_owned().map(|c|c.as_json(column)))
            .map(|cell| cell.unwrap_or_else(||json!(null)))
            .collect();
        let cells_json = json!{cells2j};
        let cells_json_text = cells_json.to_string();
        let json_md5 = Row::md5(&cells_json_text);

        let json_exists = if md5s.contains(&json_md5) {
            self.check_json_exists(conn, &cells_json_text,&json_md5).await?
        } else {
            false
        };

        if !json_exists {
            let new_row = Row{
                id:0,
                list_id: self.id,
                row_num, 
                revision_id: self.revision_id, 
                json: cells_json_text.to_owned(), 
                json_md5,
                cells,
            };
            return Ok(Some(new_row));
        }

        Ok(None)
    }

    async fn get_max_row_num(&self, conn: &mut Conn) -> Result<DbId, GenericError> {
        let list_id = self.id;
        let sql = r#"SELECT IFNULL(max(row_num),0) FROM `row` 
            WHERE revision_id=(SELECT max(revision_id) FROM `row` i WHERE i.row_num = row.row_num AND i.list_id=:list_id)
            AND list_id=:list_id"#;
        let result: Vec<DbId> = conn
            .exec_iter(sql,params! {list_id}).await?
            .map_and_drop(|row| mysql_async::from_row::<DbId>(row)).await?;
        match result.get(0) {
            Some(result) => Ok(*result),
            None => Ok(0)
        }
    }
}