use crate::data_source::*;
use crate::GulpError;
use std::fs::File;
use std::io::{self, Seek};

pub trait DataSourceAsFile {
    fn as_file(&self, ds: &DataSource) -> Result<File, GulpError>;

    fn file_from_url(&self, url: &str) -> Result<File, GulpError> {
        // TODO this should ideally by reqwest/async, but async traits are difficult the moment
        let mut reader = ureq::get(url).call()?.into_reader();
        let mut file = tempfile::tempfile()?;
        io::copy(&mut reader, &mut file)?;
        file.rewind()?;
        Ok(file)
    }
}

impl DataSourceAsFile for DataSourceTypeUrl {
    fn as_file(&self, ds: &DataSource) -> Result<File, GulpError> {
        let url = &ds.location;
        self.file_from_url(url)
    }
}

impl DataSourceAsFile for DataSourceTypeFile {
    fn as_file(&self, ds: &DataSource) -> Result<File, GulpError> {
        Ok(File::open(&ds.location)?)
    }
}

impl DataSourceAsFile for DataSourceTypePagePile {
    fn as_file(&self, ds: &DataSource) -> Result<File, GulpError> {
        let id = ds.location.parse::<usize>()?;
        let url = format!("https://pagepile.toolforge.org/api.php?id={id}&action=get_data&doit&format=json&metadata=1");
        self.file_from_url(&url)
    }
}
