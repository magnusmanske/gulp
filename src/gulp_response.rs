use http::HeaderMap;


#[derive(Debug, Clone, PartialEq)]
pub enum ContentType {
    // HTML,
    Plain,
    JSON,
    // JSONP,
    CSV,
    TSV,
}

impl ContentType {
    pub fn as_str(&self) -> &str {
        match self {
            // Self::HTML => "text/html; charset=utf-8",
            Self::Plain => "text/plain; charset=utf-8",
            Self::JSON => " application/json",
            // Self::JSONP => "application/javascript",
            Self::CSV => "text/csv; charset=utf-8",
            Self::TSV => "text/tab-separated-values; charset=utf-8",
        }
    }

    pub fn new(file_type: &str) -> Option<Self> {
        match file_type.to_lowercase().as_str() {
            "csv" => Some(Self::CSV),
            "tsv" => Some(Self::TSV),
            "json" => Some(Self::JSON),
            _ => None,
        }
    }

    pub fn download_headers(&self, filename: Option<String>) -> HeaderMap {
        let mut headers = HeaderMap::new();
        match filename {
            Some(filename) => {
                headers.insert(http::header::CONTENT_DISPOSITION, format!("attachment; filename=\"{filename}\";").parse().unwrap());
            },
            None => {
                headers.insert(http::header::CONTENT_DISPOSITION, format!("attachment;").parse().unwrap());
            },
        }
        headers.insert(http::header::CONTENT_TYPE, self.as_str().parse().unwrap());
        headers
    }

    pub fn file_ending(&self) -> String {
        match self {
            ContentType::Plain => "txt",
            ContentType::JSON => "json",
            ContentType::CSV => "csv",
            ContentType::TSV => "tsv",
        }.to_lowercase()
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct GulpResponse {
    pub s: String,
    pub content_type: ContentType,
}

