use crate::error::{Error, Result};
use super::extractor::{CodeEntity, EntityType, Language};
use std::path::Path;
use tree_sitter::{Parser, Node};

const PYTHON_EXTENSIONS: &[&str] = &["py"];
const RUST_EXTENSIONS: &[&str] = &["rs"];

pub struct CodeParser {
    language: String,
}

impl CodeParser {
    pub fn new(language: &str) -> Result<Self> {
        match language {
            "python" | "py"
            | "rust" | "rs"
            | "auto" => Ok(Self {
                language: language.to_string(),
            }),
            other => Err(Error::parser(format!(
                "Unsupported language: {other}. Use 'python', 'rust', or 'auto'"
            ))),
        }
    }

    pub fn language(&self) -> &str {
        &self.language
    }

    /// Parse source code into code entities for a supported language.
    ///
    /// We use Tree-sitter because it produces a concrete AST for Rust and
    /// Python files, which is far more reliable than regex-based parsing.
    pub fn parse_source(&self, path: &str, source: &str) -> Result<Vec<CodeEntity>> {
        let language = self.select_language(path)?;
        let mut parser = Parser::new();

        match language {
            "python" => parser
                .set_language(tree_sitter_python::language())
                .map_err(|_| Error::parser("Failed to initialize Python parser"))?,
            "rust" => parser
                .set_language(tree_sitter_rust::language())
                .map_err(|_| Error::parser("Failed to initialize Rust parser"))?,
            _ => {
                return Err(Error::parser(format!(
                    "Language not supported for parsing: {}",
                    language
                )))
            }
        }

        let tree = parser
            .parse(source, None)
            .ok_or_else(|| Error::parser("Failed to parse source code"))?;

        let root = tree.root_node();
        let entities = match language {
            "python" => self.extract_python_entities(root, source, path, ""),
            "rust" => self.extract_rust_entities(root, source, path, ""),
            _ => Vec::new(),
        };

        Ok(entities)
    }

    fn select_language(&self, path: &str) -> Result<&str> {
        if self.language == "auto" {
            let extension = Path::new(path)
                .extension()
                .and_then(|ext| ext.to_str())
                .unwrap_or_default()
                .to_lowercase();

            if PYTHON_EXTENSIONS.contains(&extension.as_str()) {
                return Ok("python");
            }
            if RUST_EXTENSIONS.contains(&extension.as_str()) {
                return Ok("rust");
            }

            Err(Error::parser(format!(
                "Could not infer language from file extension: {path}"
            )))
        } else if self.language == "py" {
            Ok("python")
        } else if self.language == "rs" {
            Ok("rust")
        } else {
            Ok(self.language.as_str())
        }
    }

    fn extract_python_entities(
        &self,
        node: Node,
        source: &str,
        path: &str,
        parent_name: &str,
    ) -> Vec<CodeEntity> {
        let mut entities = Vec::new();
        let mut cursor = node.walk();

        match node.kind() {
            "decorated_definition" => {
                if let Some(inner) = node.named_child(0) {
                    entities.extend(self.extract_python_entities(inner, source, path, parent_name));
                }
            }
            "function_definition" => {
                if let Some(name_node) = node.child_by_field_name("name") {
                    let name = self.node_text(name_node, source);
                    let full_name = if parent_name.is_empty() {
                        name.clone()
                    } else {
                        format!("{parent_name}.{name}")
                    };
                    let entity = CodeEntity::new(
                        format!("{path}::{full_name}"),
                        name,
                        if parent_name.is_empty() {
                            EntityType::Function
                        } else {
                            EntityType::Method
                        },
                        path.into(),
                        node.start_position().row as u32 + 1,
                        node.end_position().row as u32 + 1,
                        Language::Python,
                        self.extract_python_docstring(node, source),
                    );
                    entities.push(entity);
                }
            }
            "class_definition" => {
                if let Some(name_node) = node.child_by_field_name("name") {
                    let name = self.node_text(name_node, source);
                    let entity = CodeEntity::new(
                        format!("{path}::{name}"),
                        name.clone(),
                        EntityType::Class,
                        path.into(),
                        node.start_position().row as u32 + 1,
                        node.end_position().row as u32 + 1,
                        Language::Python,
                        self.extract_python_docstring(node, source),
                    );
                    entities.push(entity);

                    if let Some(body) = node
                        .named_children(&mut cursor)
                        .find(|child| child.kind() == "block")
                    {
                        for method in body.named_children(&mut body.walk()) {
                            entities.extend(self.extract_python_entities(
                                method,
                                source,
                                path,
                                &name,
                            ));
                        }
                    }
                }
            }
            _ => {
                for child in node.named_children(&mut cursor) {
                    entities.extend(self.extract_python_entities(
                        child,
                        source,
                        path,
                        parent_name,
                    ));
                }
            }
        }

        entities
    }

    fn extract_python_docstring(&self, node: Node, source: &str) -> Option<String> {
        let mut cursor = node.walk();
        let block = node
            .named_children(&mut cursor)
            .find(|child| child.kind() == "block")?;

        let first_stmt = block.named_child(0)?;
        if first_stmt.kind() != "expression_statement" {
            return None;
        }

        let expr = first_stmt.named_child(0)?;
        if expr.kind() != "string" {
            return None;
        }

        Some(self.node_text(expr, source).trim_matches('"').trim_matches('\'').to_string())
    }

    fn extract_rust_entities(
        &self,
        node: Node,
        source: &str,
        path: &str,
        parent_name: &str,
    ) -> Vec<CodeEntity> {
        let mut entities = Vec::new();
        let mut cursor = node.walk();

        match node.kind() {
            "function_item" => {
                if let Some(name_node) = node.child_by_field_name("name") {
                    let name = self.node_text(name_node, source);
                    let full_name = if parent_name.is_empty() {
                        name.clone()
                    } else {
                        format!("{parent_name}::{name}")
                    };
                    let entity = CodeEntity::new(
                        format!("{path}::{full_name}"),
                        name,
                        if parent_name.is_empty() {
                            EntityType::Function
                        } else {
                            EntityType::Method
                        },
                        path.into(),
                        node.start_position().row as u32 + 1,
                        node.end_position().row as u32 + 1,
                        Language::Rust,
                        None,
                    );
                    entities.push(entity);
                }
            }
            "impl_item" => {
                let impl_name = node
                    .child_by_field_name("type")
                    .map(|name_node| self.node_text(name_node, source))
                    .unwrap_or_default();

                for child in node.named_children(&mut cursor) {
                    entities.extend(self.extract_rust_entities(
                        child,
                        source,
                        path,
                        &impl_name,
                    ));
                }
            }
            _ => {
                for child in node.named_children(&mut cursor) {
                    entities.extend(self.extract_rust_entities(
                        child,
                        source,
                        path,
                        parent_name,
                    ));
                }
            }
        }

        entities
    }

    fn node_text(&self, node: Node, source: &str) -> String {
        source[node.start_byte()..node.end_byte()].to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn supported_languages() {
        assert!(CodeParser::new("python").is_ok());
        assert!(CodeParser::new("rs").is_ok());
        assert!(CodeParser::new("auto").is_ok());
    }

    #[test]
    fn unsupported_language() {
        assert!(CodeParser::new("cobol").is_err());
    }

    #[test]
    fn parse_python_function() {
        let source = "def foo():\n    \"doc\"\n    pass\n";
        let entities = CodeParser::new("python")
            .unwrap()
            .parse_source("test.py", source)
            .unwrap();
        assert_eq!(entities.len(), 1);
        assert_eq!(entities[0].name, "foo");
        assert_eq!(entities[0].language, Language::Python);
        assert_eq!(entities[0].docstring.as_deref(), Some("doc"));
    }

    #[test]
    fn parse_rust_function() {
        let source = "fn bar() {}\n";
        let entities = CodeParser::new("rust")
            .unwrap()
            .parse_source("lib.rs", source)
            .unwrap();
        assert_eq!(entities.len(), 1);
        assert_eq!(entities[0].name, "bar");
        assert_eq!(entities[0].language, Language::Rust);
    }
}
