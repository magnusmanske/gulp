use std::collections::HashMap;
use std::collections::HashSet;
use std::sync::Arc;
use serde::Serialize;
use mysql_async::{prelude::*, Conn};
use serde_json::json;
use wikibase::mediawiki::api::Api;
use crate::app_state::AppState;
use crate::data_source::DataSource;
use crate::data_source::DataSourceFormat;
use crate::data_source::DataSourceType;
use crate::header::*;
use crate::cell::*;
use crate::row::*;
use crate::GulpError;

const ROW_INSERT_BATCH_SIZE: usize = 1000;


#[derive(Clone, Debug, Serialize)]
pub struct List {
    pub id: DbId,
    pub name: String,
    pub revision_id: DbId, // ALWAYS THE CURRENT (LATEST) ONE
    pub header: Header,

    #[serde(skip_serializing)]
    pub app: Arc<AppState>,
}

impl List {
    pub async fn create_new(app: &Arc<AppState>, name: &str, header_schema_id: DbId) -> Option<Self> {
        let mut conn = app.get_gulp_conn().await.ok()?;
        let header_schema = HeaderSchema::from_id(&mut conn, header_schema_id).await?;

        let sql = "INSERT INTO `list` (`name`) VALUES (:name)" ;
        conn.exec_drop(sql, params!{name}).await.ok()?;
        let list_id = conn.last_insert_id()?;
        drop(conn);
        
        let mut header = Header { id: 0, list_id, revision_id: 0, schema: header_schema };
        let _ = header.create_in_db(app).await.ok()?;
        
        Self::from_id(app, list_id).await
    }

    pub async fn add_access(&self, app: &Arc<AppState>, user_id: DbId, access: &str) -> Result<(),GulpError> {
        let list_id = self.id;
        let sql = "INSERT IGNORE INTO `access` (list_id,user_id,`right`) VALUES (:list_id,:user_id,:access)";
        app.get_gulp_conn().await?.exec_drop(sql, params!{list_id,user_id,access}).await?;
        Ok(())
    }

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
        let header = Header::from_list_id(&mut conn, list_id).await?;
        Some(Self {
            app: app.clone(),
            id: row.get(0)?,
            name: row.get(1)?,
            revision_id: row.get(2)?,
            header: header,
        })
    }

    pub async fn get_rows_for_revision(&self, revision_id: DbId) -> Result<Vec<Row>, GulpError> {
        self.get_rows_for_revision_paginated(revision_id, 0, None).await
    }

    pub async fn get_rows_for_revision_paginated(&self, revision_id: DbId, start: DbId, length: Option<DbId>) -> Result<Vec<Row>, GulpError> {
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

    pub async fn get_users_in_revision(&self, revision_id: DbId) -> Result<HashMap<DbId,String>, GulpError> {
        let sql = r#"SELECT DISTINCT user_id,user.name FROM `row`,`user`
            WHERE revision_id=(SELECT max(revision_id) FROM `row` i WHERE i.row_num = row.row_num AND i.list_id=:list_id AND revision_id<=:revision_id)
            AND list_id=:list_id AND revision_id<=:revision_id AND user_id=user.id"#;
        let list_id = self.id ;
        let ret = self.app.get_gulp_conn().await?
            .exec_iter(sql,params! {list_id,revision_id}).await?
            .map_and_drop(|row| mysql_async::from_row::<(DbId,String)>(row)).await?
            .into_iter().collect();
        Ok(ret)
    }

    pub async fn get_users_by_id(&self, user_ids: &Vec<DbId>) -> Result<HashMap<DbId,String>, GulpError> {
        if user_ids.is_empty() {
            return Ok(HashMap::new());
        }
        let user_ids: Vec<String> = user_ids.iter().map(|id|format!("{id}")).collect();
        let user_ids = user_ids.join(",");
        let sql = format!("SELECT DISTINCT user.id,user.name FROM `user` WHERE id IN ({user_ids})");
        let ret = self.app.get_gulp_conn().await?
            .exec_iter(sql,()).await?
            .map_and_drop(|row| mysql_async::from_row::<(DbId,String)>(row)).await?
            .into_iter().collect();
        Ok(ret)
    }

    pub async fn get_rows_in_revision(&self, revision_id: DbId) -> Result<usize, GulpError> {
        let sql = r#"SELECT count(*) FROM `row`
            WHERE revision_id=(SELECT max(revision_id) FROM `row` i WHERE i.row_num = row.row_num AND i.list_id=:list_id AND revision_id<=:revision_id)
            AND list_id=:list_id AND revision_id<=:revision_id"#;
        let list_id = self.id ;
        let row_number = self.app.get_gulp_conn().await?
            .exec_iter(sql,params! {list_id,revision_id}).await?
            .map_and_drop(|row| mysql_async::from_row::<usize>(row)).await?.get(0).cloned().unwrap_or(0);
        Ok(row_number)
    }

    pub fn get_client() -> Result<reqwest::Client, GulpError> {
        let client = reqwest::Client::builder()
            .user_agent("gulp/0.1")
            .build()?;
        Ok(client)
    }


    pub fn get_text_from_url(url: &str) -> Result<String,GulpError> {
        let agent = ureq::AgentBuilder::new().build();
        Ok(agent.get(&url).call()?.into_string()?)
    }

    pub async fn update_from_pagepile(&self, ds: &DataSource, user_id: DbId) -> Result<(), GulpError> {
        let col_num = match self.header.schema.get_first_wiki_page_column() {
            Some(num) => num,
            None => return Err("update_from_pagepile: Wrong header schema, needs to have a WikiPage as first column".into()),
        };
        let lines = ds.get_lines(None).await?;
        let hc = match lines.headers.get(0) {
            Some(hc) => hc,
            None => return Err("update_from_pagepile: no header given from pagepile".into()),
        };
        let hc = match hc {
            Some(hc) => hc,
            None => return Err("update_from_pagepile: no wiki in header column".into()),
        };
        let wiki = match &hc.wiki {
            Some(wiki) => wiki,
            None => return Err("update_from_pagepile: no wiki in header column".into()),
        };
        let api_url = format!("https://{}/w/api.php",AppState::get_server_for_wiki(&wiki));
        let api = Api::new(&api_url).await?;

        // TODO delete rows?
        let mut conn = self.app.get_gulp_conn().await?;
        let mut md5s = self.load_json_md5s(&mut conn).await?;
        let mut next_row_num = self.get_max_row_num(&mut conn).await? + 1;
        let mut rows = vec![];
        for page in &lines.lines {
            if page.is_empty() {
                continue;
            }
            let title = wikibase::mediawiki::title::Title::new_from_full(&page, &api);
            let wp = WikiPage{ title: page.to_string(), namespace_id: Some(title.namespace_id()), wiki: Some(wiki.to_owned()) };
            let mut cells: Vec<Option<Cell>> = vec![];
            cells.resize_with(self.header.schema.columns.len(), Default::default);
            cells[col_num] = Some(Cell::WikiPage(wp));

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

    pub async fn update_from_url(&self, url: &str, file_type: DataSourceFormat, user_id: DbId) -> Result<(), GulpError> {
        let text = Self::get_client()?.get(url).send().await?.text().await?;
        let _ = match file_type {
            DataSourceFormat::JSONL => self.import_jsonl(&text, user_id).await?,
            _ => return Err("update_from_url: unsopported type {file_type}".into()),
        };
        Ok(())
    }

    async fn load_json_md5s(&self, conn: &mut Conn) -> Result<HashSet<String>,GulpError> {
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

    async fn import_jsonl(&self, text: &str, user_id: DbId) -> Result<(), GulpError> {
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

    async fn flush_row_insert(&self, conn: &mut Conn, rows: &mut Vec<Row>) -> Result<(), GulpError> {
        if rows.is_empty() {
            return Ok(());
        }
        let params: Vec<mysql_async::Params> = rows.iter().map(|row| {
            let list_id = row.list_id;
            let row_num = row.row_num;
            let revision_id = row.revision_id;
            let json = &row.json;
            let json_md5 = &row.json_md5;
            let user_id = row.user_id;
            params!{list_id,row_num,revision_id,json,json_md5,user_id}
        }).collect();
        let sql = r#"INSERT INTO `row` (list_id,row_num,revision_id,json,json_md5,user_id,modified) VALUES (:list_id,:row_num,:revision_id,:json,:json_md5,:user_id,now())"#;
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

    pub async fn get_sources(&self) -> Result<Vec<DataSource>, GulpError> {
        let list_id = self.id;
        let sql = r#"SELECT id,list_id,source_type,source_format,location,user_id FROM data_source WHERE list_id=:list_id"#;
        let sources = self.app.get_gulp_conn().await?
            .exec_iter(sql,params! {list_id}).await?
            .map_and_drop(|row| DataSource::from_row(&row)).await?
            .iter().cloned().filter_map(|s|s).collect();
        Ok(sources)
    }

    /// Checks if a revision_id increase is necessary.
    /// Returns the new current revision_id (which might be unchanged).
    pub async fn snapshot(&mut self) -> Result<DbId, GulpError> {
        let mut conn = self.app.get_gulp_conn().await?;
        // Check if there is a need to create a new snapshot, that is, increase the revision ID
        let sql = "SELECT count(id) FROM `row` WHERE list_id=:list_id AND revision_id=:revision_id" ;
        let list_id = self.id;
        let revision_id = self.revision_id;
        let results = conn
            .exec_iter(sql,params! {list_id,revision_id}).await?
            .map_and_drop(|row| mysql_async::from_row::<DbId>(row)).await?;
        let numer_of_rows_with_current_revision = results.get(0);
        let numer_of_rows_with_current_revision = match numer_of_rows_with_current_revision {
            Some(x) => *x,
            None => return Err("snapshot: database query error".into()),
        };
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

    async fn check_json_exists(&self, _conn: &mut Conn, _json_text: &str, _json_md5: &str) -> Result<bool, GulpError> {
        // Already checked via md5, might have to implement if collisions occur
        Ok(true)
    }

    async fn get_or_ignore_new_row(&self, conn: &mut Conn, md5s: &HashSet<String>, cells: Vec<Option<Cell>>, row_num: DbId, user_id: DbId) -> Result<Option<Row>, GulpError> {
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

    pub async fn update_from_source(&self, source: &DataSource, user_id: DbId) -> Result<(),GulpError> {
        match source.source_type {
            DataSourceType::URL => self.update_from_url(&source.location,source.source_format.to_owned(), user_id).await?,
            DataSourceType::FILE => todo!(),
            DataSourceType::PAGEPILE => self.update_from_pagepile(&source, user_id).await?,
        }
        Ok(())
    }

    async fn get_max_row_num(&self, conn: &mut Conn) -> Result<DbId, GulpError> {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app_state::*;

    #[tokio::test]
    async fn test_from_id() {
        let app = AppState::from_config_file("config.json").expect("app creation failed");
        let app = Arc::new(app);
        let list = List::from_id(&app, 4).await.expect("from_id fail");
        assert_eq!(list.id,4);
        assert_eq!(list.name,"File candidates Hessen");
        println!("{:?}",list.header.schema.columns[0]);
        //assert_eq!(list.header.schema.columns[0],"File candidates Hessen");
    }
}