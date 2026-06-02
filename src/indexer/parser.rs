use crate::error::{Error, Result};
use super::extractor::{CodeEntity, EntityType, Language};
use std::path::Path;
use tree_sitter::{Parser, Node};

const PYTHON_EXTENSIONS: &[&str] = &["py"];
const RUST_EXTENSIONS: &[&str] = &["rs"];

#[derive(Debug, Clone)]
pub struct Import {
    pub source_file: String,
    pub module_path: String,
    pub symbol: Option<String>,
    pub alias: Option<String>,
    pub imported_name: String,
}

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

    /// Extract imports from source code (Python: import/from statements, Rust: use statements).
    pub fn extract_imports(&self, path: &str, source: &str) -> Result<Vec<Import>> {
        let language = self.select_language(path)?;
        let mut parser = Parser::new();

        match language {
            "python" => parser
                .set_language(tree_sitter_python::language())
                .map_err(|_| Error::parser("Failed to initialize Python parser"))?,
            "rust" => parser
                .set_language(tree_sitter_rust::language())
                .map_err(|_| Error::parser("Failed to initialize Rust parser"))?,
            _ => return Ok(Vec::new()),
        }

        let tree = parser
            .parse(source, None)
            .ok_or_else(|| Error::parser("Failed to parse source code"))?;

        let root = tree.root_node();
        let imports = match language {
            "python" => self.extract_python_imports(root, source, path),
            "rust" => self.extract_rust_imports(root, source, path),
            _ => Vec::new(),
        };

        Ok(imports)
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

        // First pass: collect entities with their defining node so we can scan
        // each entity body for call-sites and resolve intra-file edges.
        let local_entities_nodes: Vec<(CodeEntity, Node)> = match language {
            "python" => self.collect_python_entities(root, source, path, ""),
            "rust" => self.collect_rust_entities(root, source, path, ""),
            _ => Vec::new(),
        };

        // Build name -> id map for quick resolution within the same file.
        let mut name_map: std::collections::HashMap<String, Vec<String>> = std::collections::HashMap::new();
        for (e, _) in &local_entities_nodes {
            name_map.entry(e.name.clone()).or_default().push(e.id.clone());
        }

        // Second pass: scan each entity node for call expressions and populate calls
        let mut resolved_entities: Vec<CodeEntity> = Vec::new();
        for (mut e, node) in local_entities_nodes {
            let callees = match language {
                "python" => self.find_python_calls(node, source),
                "rust" => self.find_rust_calls(node, source),
                _ => Vec::new(),
            };

            for callee_name in callees {
                if let Some(ids) = name_map.get(&callee_name) {
                    for id in ids {
                        e.add_call(id.clone());
                    }
                } else if let Some(last_name) = callee_name
                    .split(|c: char| c == '.' || c == ':')
                    .filter(|s| !s.is_empty())
                    .last()
                {
                    if let Some(ids) = name_map.get(last_name) {
                        for id in ids {
                            e.add_call(id.clone());
                        }
                        continue;
                    }
                    e.add_call(callee_name);
                } else {
                    e.add_call(callee_name);
                }
            }

            resolved_entities.push(e);
        }

        Ok(resolved_entities)
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

    fn collect_python_entities<'a>(
        &self,
        node: Node<'a>,
        source: &str,
        path: &str,
        parent_name: &str,
    ) -> Vec<(CodeEntity, Node<'a>)> {
        let mut entities: Vec<(CodeEntity, Node)> = Vec::new();
        let mut cursor = node.walk();

        match node.kind() {
            "decorated_definition" => {
                if let Some(inner) = node.named_child(0) {
                    entities.extend(self.collect_python_entities(inner, source, path, parent_name));
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
                    entities.push((entity, node));
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
                    entities.push((entity, node));

                    if let Some(body) = node
                        .named_children(&mut cursor)
                        .find(|child| child.kind() == "block")
                    {
                        for method in body.named_children(&mut body.walk()) {
                            entities.extend(self.collect_python_entities(
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
                    entities.extend(self.collect_python_entities(
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

    fn find_python_calls<'a>(&self, node: Node<'a>, source: &str) -> Vec<String> {
        let mut names = Vec::new();
        let mut cursor = node.walk();

        for child in node.named_children(&mut cursor) {
            if child.kind() == "call" {
                if let Some(first) = child.named_child(0) {
                    let txt = self.node_text(first, source).trim().to_string();
                    if !txt.is_empty() {
                        names.push(txt);
                    }
                }
            }
            names.extend(self.find_python_calls(child, source));
        }

        names
    }

    fn collect_rust_entities<'a>(
        &self,
        node: Node<'a>,
        source: &str,
        path: &str,
        parent_name: &str,
    ) -> Vec<(CodeEntity, Node<'a>)> {
        let mut entities: Vec<(CodeEntity, Node)> = Vec::new();
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
                    entities.push((entity, node));
                }
            }
            "impl_item" => {
                let impl_name = node
                    .child_by_field_name("type")
                    .map(|name_node| self.node_text(name_node, source))
                    .unwrap_or_default();

                for child in node.named_children(&mut cursor) {
                    entities.extend(self.collect_rust_entities(
                        child,
                        source,
                        path,
                        &impl_name,
                    ));
                }
            }
            _ => {
                for child in node.named_children(&mut cursor) {
                    entities.extend(self.collect_rust_entities(
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

    fn find_rust_calls<'a>(&self, node: Node<'a>, source: &str) -> Vec<String> {
        let mut names = Vec::new();
        let mut cursor = node.walk();

        for child in node.named_children(&mut cursor) {
            if child.kind() == "call_expression" {
                if let Some(func) = child.named_child(0) {
                    let txt = self.node_text(func, source).trim().to_string();
                    if !txt.is_empty() {
                        names.push(txt);
                    }
                }
            }
            names.extend(self.find_rust_calls(child, source));
        }

        names
    }
    fn extract_python_imports(
        &self,
        node: Node,
        source: &str,
        file: &str,
    ) -> Vec<Import> {
        let mut imports = Vec::new();
        let mut cursor = node.walk();

        match node.kind() {
            "import_statement" => {
                for child in node.named_children(&mut cursor) {
                    if child.kind() == "dotted_name" || child.kind() == "identifier" {
                        let module_path = self.node_text(child, source);
                        let imported_name = module_path
                            .split('.')
                            .next()
                            .unwrap_or_default()
                            .to_string();
                        imports.push(Import {
                            source_file: file.to_string(),
                            module_path,
                            symbol: None,
                            alias: None,
                            imported_name,
                        });
                    } else if child.kind() == "import_alias" {
                        if let Some(first) = child.named_child(0) {
                            let module_path = self.node_text(first, source);
                            let alias = child.child_by_field_name("alias")
                                .map(|n| self.node_text(n, source));
                            let imported_name = alias.clone().unwrap_or_else(|| {
                                module_path
                                    .split('.')
                                    .next()
                                    .unwrap_or_default()
                                    .to_string()
                            });
                            imports.push(Import {
                                source_file: file.to_string(),
                                module_path,
                                symbol: None,
                                alias,
                                imported_name,
                            });
                        }
                    }
                }
            }
            "import_from_statement" => {
                let mut module_path = String::new();
                let mut imports_list = Vec::new();

                for child in node.named_children(&mut cursor) {
                    if child.kind() == "dotted_name" || child.kind() == "identifier" {
                        module_path = self.node_text(child, source);
                    } else if child.kind() == "import_list" {
                        let mut import_cursor = child.walk();
                        for imp in child.named_children(&mut import_cursor) {
                            if imp.kind() == "import_alias" || imp.kind() == "identifier" {
                                if let Some(first) = imp.named_child(0) {
                                    let symbol = self.node_text(first, source);
                                    let alias = imp.child_by_field_name("alias")
                                        .map(|n| self.node_text(n, source));
                                    imports_list.push((symbol, alias));
                                }
                            }
                        }
                    }
                }

                for (symbol, alias) in imports_list {
                    let imported_name = alias.clone().unwrap_or_else(|| symbol.clone());
                    imports.push(Import {
                        source_file: file.to_string(),
                        module_path: module_path.clone(),
                        symbol: Some(symbol),
                        alias,
                        imported_name,
                    });
                }
            }
            _ => {
                for child in node.named_children(&mut cursor) {
                    imports.extend(self.extract_python_imports(child, source, file));
                }
            }
        }

        imports
    }

    fn extract_rust_imports(
        &self,
        node: Node,
        source: &str,
        file: &str,
    ) -> Vec<Import> {
        let mut imports = Vec::new();
        let mut cursor = node.walk();

        match node.kind() {
            "use_declaration" => {
                imports.extend(self.parse_rust_use_declaration(node, source, file));
            }
            _ => {
                for child in node.named_children(&mut cursor) {
                    imports.extend(self.extract_rust_imports(child, source, file));
                }
            }
        }

        imports
    }

    fn parse_rust_use_declaration(
        &self,
        node: Node,
        source: &str,
        file: &str,
    ) -> Vec<Import> {
        let mut use_text = self.node_text(node, source).trim().to_string();
        if let Some(rest) = use_text.strip_prefix("use") {
            use_text = rest.trim().to_string();
        }
        if use_text.ends_with(';') {
            use_text = use_text[..use_text.len() - 1].trim().to_string();
        }
        self.parse_rust_use_path(&use_text, file)
    }

    fn parse_rust_use_path(&self, path_text: &str, file: &str) -> Vec<Import> {
        let mut imports = Vec::new();
        let path_text = path_text.trim();

        if let Some(start) = path_text.find('{') {
            let mut depth = 0;
            let mut last_sep = 0;
            let prefix = path_text[..start].trim_end_matches("::").trim();
            let items_start = start + 1;
            let mut items = Vec::new();
            for (idx, ch) in path_text[items_start..].char_indices() {
                match ch {
                    '{' => depth += 1,
                    '}' if depth > 0 => depth -= 1,
                    '}' => {
                        items.push(path_text[items_start..items_start + idx].trim());
                        break;
                    }
                    ',' if depth == 0 => {
                        items.push(path_text[items_start + last_sep..items_start + idx].trim());
                        last_sep = idx + 1;
                    }
                    _ => {}
                }
            }

            for item in items {
                if item.contains('{') {
                    let nested_prefix = item
                        .split_once('{')
                        .map(|(left, _)| left.trim_end_matches("::").trim())
                        .unwrap_or(item.trim());
                    let nested_body = item
                        .split_once('{')
                        .and_then(|(_, right)| right.strip_suffix('}'))
                        .unwrap_or(""
                        );
                    let full_prefix = if prefix.is_empty() {
                        nested_prefix.to_string()
                    } else {
                        format!("{prefix}::{nested_prefix}")
                    };
                    let nested_path = format!("{full_prefix}::{{{}}}", nested_body);
                    imports.extend(self.parse_rust_use_path(&nested_path, file));
                } else {
                    let (item_path, alias) = self.parse_rust_use_alias(item);
                    let mut full_path = if prefix.is_empty() {
                        item_path
                    } else {
                        format!("{prefix}::{item_path}")
                    };
                    if let Some(alias) = alias {
                        full_path = format!("{full_path} as {alias}");
                    }
                    imports.extend(self.parse_rust_use_path(&full_path, file));
                }
            }

            return imports;
        }

        let (path, alias) = self.parse_rust_use_alias(path_text);
        let imported_name = alias.clone().unwrap_or_else(|| {
            path.split("::").last().unwrap_or_default().to_string()
        });
        let path_segments: Vec<&str> = path.split("::").collect();
        let symbol = path_segments.last().map(|s| s.to_string());
        let module_path = if path_segments.len() > 1 {
            path_segments[..path_segments.len() - 1].join("::")
        } else {
            path.to_string()
        };

        imports.push(Import {
            source_file: file.to_string(),
            module_path,
            symbol,
            alias,
            imported_name,
        });
        imports
    }

    fn parse_rust_use_alias(&self, path_text: &str) -> (String, Option<String>) {
        let path_text = path_text.trim();
        if let Some((left, right)) = path_text.split_once(" as ") {
            (left.trim().to_string(), Some(right.trim().to_string()))
        } else {
            (path_text.to_string(), None)
        }
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
