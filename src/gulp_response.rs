
#[derive(Debug, Clone, PartialEq)]
pub enum ContentType {
    HTML,
    Plain,
    JSON,
    JSONP,
    CSV,
    TSV,
}

impl ContentType {
    pub fn as_str(&self) -> &str {
        match self {
            Self::HTML => "text/html; charset=utf-8",
            Self::Plain => "text/plain; charset=utf-8",
            Self::JSON => " application/json",
            Self::JSONP => "application/javascript",
            Self::CSV => "text/csv; charset=utf-8",
            Self::TSV => "text/tab-separated-values; charset=utf-8",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct GulpResponse {
    pub s: String,
    pub content_type: ContentType,
}

