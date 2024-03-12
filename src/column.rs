use serde::Serialize;
use std::str::FromStr;

#[derive(Debug, PartialEq, Eq)]
pub struct ParsColumnTypeError;

#[derive(Clone, Debug, PartialEq, Serialize)]
pub enum ColumnType {
    String,
    WikiPage,
    Location,
}
impl FromStr for ColumnType {
    type Err = ParsColumnTypeError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "String" => Ok(Self::String),
            "WikiPage" => Ok(Self::WikiPage),
            "Location" => Ok(Self::Location),
            _ => Err(ParsColumnTypeError),
        }
    }
}
