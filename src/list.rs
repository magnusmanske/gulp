use std::collections::HashSet;
use std::sync::Arc;
use serde::Serialize;
use mysql_async::{prelude::*, Conn};
use serde_json::json;
use crate::app_state::AppState;
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

#[derive(Clone, Debug, Serialize)]
pub struct List {
    pub id: DbId,
    pub name: String,
    pub revision_id: DbId, // ALWAYS THE CURRENT ONE

    #[serde(skip_serializing)]
    pub header: Header,

    #[serde(skip_serializing)]
    pub app: Arc<AppState>,
}

impl List {
    pub async fn from_id(app: &Arc<AppState>, list_id: DbId) -> Option<Self> {
        let sql = r#"SELECT id,name,revision_id FROM `list` WHERE id=:list_id"#;
        let row = app.get_gulp_conn().await.ok()?
            .exec_iter(sql,params! {list_id}).await.ok()?
            .map_and_drop(|row| row).await.ok()?
            .get(0)?.to_owned();
        Self::from_row(app, &row, list_id).await
    }

    pub async fn from_row(app: &Arc<AppState>, row: &mysql_async::Row, list_id: DbId) -> Option<Self> {
        let mut conn = app.get_gulp_conn().await.ok()?;
        Some(Self {
            app: app.clone(),
            id: row.get(0)?,
            name: row.get(1)?,
            revision_id: row.get(2)?,
            header: Header::from_list_id(&mut conn, list_id).await?,
        })
    }

    pub async fn get_rows_for_revision(&self, revision_id: DbId) -> Result<Vec<Row>, GenericError> {
        self.get_rows_for_revision_paginated(revision_id, 0, None).await
    }

    pub async fn get_rows_for_revision_paginated(&self, revision_id: DbId, start: DbId, length: Option<DbId>) -> Result<Vec<Row>, GenericError> {
        let length = length.unwrap_or(DbId::MAX);
        let list_id = self.id ;
        let sql = r#"SELECT row.id,list_id,row_num,revision_id,json,json_md5,user_id,modified
            FROM `row`
            WHERE revision_id=(SELECT max(revision_id) FROM `row` i WHERE i.row_num = row.row_num AND i.list_id=:list_id AND revision_id<=:revision_id)
            AND list_id=:list_id AND revision_id<=:revision_id
            ORDER BY row_num
            LIMIT :length OFFSET :start"#;
        let row_opts = self.app.get_gulp_conn().await?
            .exec_iter(sql,params! {list_id,revision_id,start,length}).await?
            .map_and_drop(|row| Row::from_row(&row,&self.header)).await?;
        let rows: Vec<Row> = row_opts.iter().cloned().filter_map(|row|row).collect();
        Ok(rows)
    }

    pub async fn import_from_url(&self, url: &str, file_type: FileType, user_id: DbId) -> Result<(), GenericError> {
        let client = reqwest::Client::builder()
            .user_agent("gulp/0.1")
            .build()?;
        let text = client.get(url).send().await?.text().await?;
        let _ = match file_type {
            FileType::JSONL => self.import_jsonl(&text, user_id).await?,
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

    async fn import_jsonl(&self, text: &str, user_id: DbId) -> Result<(), GenericError> {
        // TODO delete rows?
        let mut conn = self.app.get_gulp_conn().await?;
        let mut md5s = self.load_json_md5s(&mut conn).await?;
        let mut next_row_num = self.get_max_row_num(&mut conn).await? + 1;
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
            if let Some(row) = self.get_or_ignore_new_row(&mut conn, &md5s, cells, next_row_num, user_id).await? {
                next_row_num += 1;
                md5s.insert(row.json_md5.to_owned());
                rows.push(row);
                if rows.len()>=ROW_INSERT_BATCH_SIZE {
                    self.flush_row_insert(&mut conn, &mut rows).await?;
                }
            }
        }
        self.flush_row_insert(&mut conn, &mut rows).await?;
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

    /// Checks if a revision_id increase is necessary.
    /// Returns the new current revision_id (which might be unchanged).
    pub async fn snapshot(&mut self) -> Result<DbId, GenericError> {
        let mut conn = self.app.get_gulp_conn().await?;
        // Check if there is a need to create a new snapshot, that is, increase the revision ID
        let sql = "SELECT count(id) FROM `row` WHERE list_id=:list_id AND revision_id=:revision_id" ;
        let list_id = self.id;
        let revision_id = self.revision_id;
        let numer_of_rows_with_current_revision = *conn
            .exec_iter(sql,params! {list_id,revision_id}).await?
            .map_and_drop(|row| mysql_async::from_row::<DbId>(row)).await?.get(0).unwrap();
        if numer_of_rows_with_current_revision==0 { // No need to make a new snapshot
            return Ok(self.revision_id);
        }

        // Create new revision ID
        self.revision_id += 1 ;
        let sql = "UPDATE `list` SET revision_id=:revision_id WHERE id=:list_id" ;
        let list_id = self.id;
        let revision_id = self.revision_id;
        conn.exec_drop(sql, params!{list_id,revision_id}).await?;

        Ok(self.revision_id)
    }

    async fn check_json_exists(&self, _conn: &mut Conn, _json_text: &str, _json_md5: &str) -> Result<bool, GenericError> {
        // Already checked via md5, might have to implement if collisions occur
        /*
            let list_id = self.id;
            let sql = "SELECT id FROM `row`
                WHERE revision_id=(SELECT max(revision_id) FROM `row` i WHERE i.row_num = row.row_num AND i.list_id=:list_id)
                AND list_id=:list_id
                AND json_md5=:json_md5
                AND json=:json_text";
            Ok(!conn
                .exec_iter(sql,params! {list_id,json_text,json_md5}).await?
                .map_and_drop(|_row| 1).await?.is_empty())
         */
        Ok(true)
    }

    async fn get_or_ignore_new_row(&self, conn: &mut Conn, md5s: &HashSet<String>, cells: Vec<Option<Cell>>, row_num: DbId, user_id: DbId) -> Result<Option<Row>, GenericError> {
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
                user_id,
                modified: String::new(),
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