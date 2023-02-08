use mysql_async::{prelude::*, Conn};
use async_session::{Result, Session, SessionStore};
use async_trait::async_trait;

#[derive(Default, Debug, Clone)]
pub struct DatabaseSessionStore {
    pub pool: Option<mysql_async::Pool>
}

#[async_trait]
impl SessionStore for DatabaseSessionStore {
    async fn load_session(&self, cookie_value: String) -> Result<Option<Session>> {
        let id = Session::id_from_cookie_value(&cookie_value)?;
        println!("{id} => {cookie_value}");
        let sql = "SELECT `cookie` FROM `session` WHERE `id_string`=:id" ;
        let res = self.get_gulp_conn().await
            .exec_iter(sql,params! {id}).await?
            .map_and_drop(|row| mysql_async::from_row::<String>(row)).await.unwrap().get(0).cloned();
        match res {
            Some(s) => {
                let mut session = Session::new();
                session.set_cookie_value(s);
                // TODO Session::validate
                Ok(Some(session))
            }
            None => return Ok(None)
        }
    }

    async fn store_session(&self, session: Session) -> Result<Option<String>> {
        let id = session.id().to_string();
        let cookie = session.clone().into_cookie_value().unwrap();
        let sql = "REPLACE INTO `session` (id_string,cookie) VALUES (:id,:cookie)" ;
        self.get_gulp_conn().await.exec_drop(sql, params!{id,cookie}).await?;

        session.reset_data_changed();
        Ok(session.into_cookie_value())
    }

    async fn destroy_session(&self, session: Session) -> Result {
        let id = session.id().to_string();
        let sql = "DELETE FROM `session` WHERE `id_string`=:id" ;
        self.get_gulp_conn().await.exec_drop(sql, params!{id}).await?;
        Ok(())
    }

    async fn clear_store(&self) -> Result {
        let id = 0;
        let sql = "DELETE FROM `session` WHERE id>:id" ;
        self.get_gulp_conn().await.exec_drop(sql, params!{id}).await?;
        Ok(())
    }
}

impl DatabaseSessionStore {
    /// Create a new instance of DatabaseSessionStore
    pub fn new() -> Self {
        Self::default()
    }

    async fn get_gulp_conn(&self) -> Conn {
        self.pool.as_ref().unwrap().get_conn().await.unwrap()
    }

    /// Performs session cleanup. This should be run on an
    /// intermittent basis if this store is run for long enough that
    /// memory accumulation is a concern
    pub async fn cleanup(&self) -> Result {
        // TODO delete expired? session.is_expired()
        Ok(())
    }

    /// returns the number of elements in the memory store
    pub async fn count(&self) -> usize {
        let id = 0;
        let sql = "SELECT count(*) FROM `session` WHERE id>:id" ;
        *self.get_gulp_conn().await
            .exec_iter(sql,params! {id}).await.unwrap()
            .map_and_drop(|row| mysql_async::from_row::<usize>(row)).await.unwrap().get(0).unwrap()
    }
}
