use crate::error::{Error, Result};
use super::extractor::CodeEntity;

pub struct CodeParser {
    language: String,
}

impl CodeParser {
    pub fn new(language: &str) -> Result<Self> {
        match language {
            "python" | "py"
            | "javascript" | "js"
            | "typescript" | "ts"
            | "rust" | "rs"
            | "go" | "auto" => Ok(Self {
                language: language.to_string(),
            }),
            other => Err(Error::parser(format!(
                "Unsupported language: {other}. Use 'python', 'javascript', 'rust', 'go', or 'auto'"
            ))),
        }
    }

    pub fn language(&self) -> &str {
        &self.language
    }

    /// Parse source code and return extracted entities.
    /// TODO: Implement with tree-sitter in Part 1.
    pub fn parse_source(&self, _path: &str, _source: &str) -> Result<Vec<CodeEntity>> {
        Ok(Vec::new())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn supported_languages() {
        assert!(CodeParser::new("python").is_ok());
        assert!(CodeParser::new("js").is_ok());
        assert!(CodeParser::new("typescript").is_ok());
        assert!(CodeParser::new("rust").is_ok());
        assert!(CodeParser::new("auto").is_ok());
    }

    #[test]
    fn unsupported_language() {
        assert!(CodeParser::new("cobol").is_err());
    }
}
