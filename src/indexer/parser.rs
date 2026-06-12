use super::extractor::{CodeEntity, EntityType, Language};
use crate::error::{Error, Result};
use std::path::Path;
use tree_sitter::{Node, Parser};

const PYTHON_EXTENSIONS: &[&str] = &["py"];
const RUST_EXTENSIONS: &[&str] = &["rs"];
const JAVASCRIPT_EXTENSIONS: &[&str] = &["js", "jsx", "mjs", "cjs"];
const TYPESCRIPT_EXTENSIONS: &[&str] = &["ts", "tsx", "mts", "cts"];
const GO_EXTENSIONS: &[&str] = &["go"];

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
            | "rust"   | "rs"
            | "javascript" | "js"
            | "typescript" | "ts"
            | "go"
            | "auto" => Ok(Self {
                language: language.to_string(),
            }),
            other => Err(Error::parser(format!(
                "Unsupported language: {other}. Use 'python', 'rust', 'javascript', 'typescript', 'go', or 'auto'"
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
            "javascript" => parser
                .set_language(tree_sitter_javascript::language())
                .map_err(|_| Error::parser("Failed to initialize JavaScript parser"))?,
            "typescript" => parser
                .set_language(tree_sitter_typescript::language_typescript())
                .map_err(|_| Error::parser("Failed to initialize TypeScript parser"))?,
            "go" => parser
                .set_language(tree_sitter_go::language())
                .map_err(|_| Error::parser("Failed to initialize Go parser"))?,
            _ => return Ok(Vec::new()),
        }

        let tree = parser
            .parse(source, None)
            .ok_or_else(|| Error::parser("Failed to parse source code"))?;

        let root = tree.root_node();
        let imports = match language {
            "python" => self.extract_python_imports(root, source, path),
            "rust" => self.extract_rust_imports(root, source, path),
            "javascript" | "typescript" => self.extract_js_imports(root, source, path),
            "go" => self.extract_go_imports(root, source, path),
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
            "javascript" => parser
                .set_language(tree_sitter_javascript::language())
                .map_err(|_| Error::parser("Failed to initialize JavaScript parser"))?,
            "typescript" => parser
                .set_language(tree_sitter_typescript::language_typescript())
                .map_err(|_| Error::parser("Failed to initialize TypeScript parser"))?,
            "go" => parser
                .set_language(tree_sitter_go::language())
                .map_err(|_| Error::parser("Failed to initialize Go parser"))?,
            _ => {
                return Err(Error::parser(format!(
                    "Language not supported for parsing: {language}"
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
            "javascript" => self.collect_js_entities(root, source, path, Language::JavaScript),
            "typescript" => self.collect_js_entities(root, source, path, Language::TypeScript),
            "go" => self.collect_go_entities(root, source, path),
            _ => Vec::new(),
        };

        // Build name -> id map for quick resolution within the same file.
        let mut name_map: std::collections::HashMap<String, Vec<String>> =
            std::collections::HashMap::new();
        for (e, _) in &local_entities_nodes {
            name_map
                .entry(e.name.clone())
                .or_default()
                .push(e.id.clone());
        }

        // Second pass: scan each entity node for call expressions and populate calls
        let mut resolved_entities: Vec<CodeEntity> = Vec::new();
        for (mut e, node) in local_entities_nodes {
            let callees = match language {
                "python" => self.find_python_calls(node, source),
                "rust" => self.find_rust_calls(node, source),
                "javascript" | "typescript" => self.find_js_calls(node, source),
                "go" => self.find_go_calls(node, source),
                _ => Vec::new(),
            };

            for callee_name in callees {
                if let Some(ids) = name_map.get(&callee_name) {
                    for id in ids {
                        e.add_call(id.clone());
                    }
                } else if let Some(last_name) = callee_name
                    .split(['.', ':'])
                    .rfind(|s: &&str| !s.is_empty())
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
            if JAVASCRIPT_EXTENSIONS.contains(&extension.as_str()) {
                return Ok("javascript");
            }
            if TYPESCRIPT_EXTENSIONS.contains(&extension.as_str()) {
                return Ok("typescript");
            }
            if GO_EXTENSIONS.contains(&extension.as_str()) {
                return Ok("go");
            }

            Err(Error::parser(format!(
                "Could not infer language from file extension: {path}"
            )))
        } else if self.language == "py" {
            Ok("python")
        } else if self.language == "rs" {
            Ok("rust")
        } else if self.language == "js" {
            Ok("javascript")
        } else if self.language == "ts" {
            Ok("typescript")
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
                    let entity_type = if is_test_name(&name) {
                        EntityType::TestFunction
                    } else if parent_name.is_empty() {
                        EntityType::Function
                    } else {
                        EntityType::Method
                    };
                    let entity = CodeEntity::new(
                        format!("{path}::{full_name}"),
                        name,
                        entity_type,
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
                            entities
                                .extend(self.collect_python_entities(method, source, path, &name));
                        }
                    }
                }
            }
            _ => {
                for child in node.named_children(&mut cursor) {
                    entities.extend(self.collect_python_entities(child, source, path, parent_name));
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

        Some(
            self.node_text(expr, source)
                .trim_matches('"')
                .trim_matches('\'')
                .to_string(),
        )
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
                    let entity_type = if has_rust_test_attribute(node, source) {
                        EntityType::TestFunction
                    } else if parent_name.is_empty() {
                        EntityType::Function
                    } else {
                        EntityType::Method
                    };
                    let entity = CodeEntity::new(
                        format!("{path}::{full_name}"),
                        name,
                        entity_type,
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
                    entities.extend(self.collect_rust_entities(child, source, path, &impl_name));
                }
            }
            _ => {
                for child in node.named_children(&mut cursor) {
                    entities.extend(self.collect_rust_entities(child, source, path, parent_name));
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

    // ── Go ─────────────────────────────────────────────────────────────────────

    /// Collects entities from a Go AST.
    ///
    /// Handles:
    ///   function_declaration → Function or TestFunction (see `has_go_test_signature`)
    ///   method_declaration   → Method, id is `path::ReceiverType::method_name`
    ///                           (mirrors Rust's `StructName::method_name`)
    ///
    /// Go has no nested named function declarations (closures assigned to
    /// variables aren't tracked as entities, same scope decision as JS arrow
    /// functions assigned to object properties), so no `parent_name` threading
    /// is needed -every entity is found directly under `source_file`.
    fn collect_go_entities<'a>(
        &self,
        node: Node<'a>,
        source: &str,
        path: &str,
    ) -> Vec<(CodeEntity, Node<'a>)> {
        let mut entities: Vec<(CodeEntity, Node)> = Vec::new();
        let mut cursor = node.walk();

        match node.kind() {
            "function_declaration" => {
                if let Some(name_node) = node.child_by_field_name("name") {
                    let name = self.node_text(name_node, source);
                    let entity_type = if has_go_test_signature(node, &name, source) {
                        EntityType::TestFunction
                    } else {
                        EntityType::Function
                    };
                    let entity = CodeEntity::new(
                        format!("{path}::{name}"),
                        name,
                        entity_type,
                        path.into(),
                        node.start_position().row as u32 + 1,
                        node.end_position().row as u32 + 1,
                        Language::Go,
                        None,
                    );
                    entities.push((entity, node));
                }
            }
            "method_declaration" => {
                if let Some(name_node) = node.child_by_field_name("name") {
                    let name = self.node_text(name_node, source);
                    let full_name = match go_receiver_type(node, source) {
                        Some(receiver) => format!("{receiver}::{name}"),
                        None => name.clone(),
                    };
                    let entity = CodeEntity::new(
                        format!("{path}::{full_name}"),
                        name,
                        EntityType::Method,
                        path.into(),
                        node.start_position().row as u32 + 1,
                        node.end_position().row as u32 + 1,
                        Language::Go,
                        None,
                    );
                    entities.push((entity, node));
                }
            }
            _ => {
                for child in node.named_children(&mut cursor) {
                    entities.extend(self.collect_go_entities(child, source, path));
                }
            }
        }

        entities
    }

    /// Finds call expressions in a Go node subtree.
    ///
    /// `call_expression.function` is either an `identifier` (`helper()`) or a
    /// `selector_expression` (`pkg.Helper()`, `r.Method()`) -for the latter,
    /// `node_text` yields the dotted text (e.g. "r.Method"). The name-resolution
    /// pass in `parse_source` already falls back to the last `.`/`::`-separated
    /// segment when an exact match isn't found, so "r.Method" still resolves to
    /// a local method named "Method".
    fn find_go_calls<'a>(&self, node: Node<'a>, source: &str) -> Vec<String> {
        let mut names = Vec::new();
        let mut cursor = node.walk();

        for child in node.named_children(&mut cursor) {
            if child.kind() == "call_expression" {
                if let Some(func) = child.child_by_field_name("function") {
                    let txt = self.node_text(func, source).trim().to_string();
                    if !txt.is_empty() {
                        names.push(txt);
                    }
                }
            }
            names.extend(self.find_go_calls(child, source));
        }

        names
    }

    /// Extracts Go import declarations: both `import "fmt"` and grouped
    /// `import (\n  "fmt"\n  myalias "path/to/pkg"\n)`.
    fn extract_go_imports(&self, node: Node, source: &str, file: &str) -> Vec<Import> {
        let mut imports = Vec::new();
        let mut cursor = node.walk();

        match node.kind() {
            "import_declaration" => {
                for child in node.named_children(&mut cursor) {
                    match child.kind() {
                        "import_spec" => {
                            if let Some(imp) = self.parse_go_import_spec(child, source, file) {
                                imports.push(imp);
                            }
                        }
                        "import_spec_list" => {
                            let mut list_cursor = child.walk();
                            for spec in child.named_children(&mut list_cursor) {
                                if spec.kind() == "import_spec" {
                                    if let Some(imp) = self.parse_go_import_spec(spec, source, file)
                                    {
                                        imports.push(imp);
                                    }
                                }
                            }
                        }
                        _ => {}
                    }
                }
            }
            _ => {
                for child in node.named_children(&mut cursor) {
                    imports.extend(self.extract_go_imports(child, source, file));
                }
            }
        }

        imports
    }

    /// Parses a single `import_spec`: `"fmt"` or `myalias "path/to/pkg"`.
    ///
    /// `imported_name` is the alias if present, otherwise the last
    /// `/`-separated segment of the import path (Go's default package name
    /// convention, e.g. `"path/to/pkg"` → `pkg`).
    fn parse_go_import_spec(&self, node: Node, source: &str, file: &str) -> Option<Import> {
        let path_node = node.child_by_field_name("path")?;
        let module_path = self
            .node_text(path_node, source)
            .trim_matches('"')
            .to_string();
        let alias = node
            .child_by_field_name("name")
            .map(|n| self.node_text(n, source));
        let imported_name = alias.clone().unwrap_or_else(|| {
            module_path
                .rsplit('/')
                .next()
                .unwrap_or(&module_path)
                .to_string()
        });

        Some(Import {
            source_file: file.to_string(),
            module_path,
            symbol: None,
            alias,
            imported_name,
        })
    }

    // ── JavaScript / TypeScript ───────────────────────────────────────────────

    /// Collects entities from a JS/TS AST.
    ///
    /// Handles:
    ///   function_declaration     → Function
    ///   arrow_function (const f = () => {}) → Function
    ///   method_definition        → Method
    ///   class_declaration        → Class
    fn collect_js_entities<'a>(
        &self,
        node: Node<'a>,
        source: &str,
        path: &str,
        lang: Language,
    ) -> Vec<(CodeEntity, Node<'a>)> {
        let mut entities: Vec<(CodeEntity, Node)> = Vec::new();
        let mut cursor = node.walk();

        match node.kind() {
            "function_declaration" | "generator_function_declaration" => {
                if let Some(name_node) = node.child_by_field_name("name") {
                    let name = self.node_text(name_node, source);
                    let entity_type = if is_test_name(&name) {
                        EntityType::TestFunction
                    } else {
                        EntityType::Function
                    };
                    let entity = CodeEntity::new(
                        format!("{path}::{name}"),
                        name,
                        entity_type,
                        path.into(),
                        node.start_position().row as u32 + 1,
                        node.end_position().row as u32 + 1,
                        lang,
                        None,
                    );
                    entities.push((entity, node));
                }
            }
            "lexical_declaration" | "variable_declaration" => {
                // const foo = () => { ... }  or  const foo = function() { ... }
                for child in node.named_children(&mut cursor) {
                    if child.kind() == "variable_declarator" {
                        let name_opt = child
                            .child_by_field_name("name")
                            .map(|n| self.node_text(n, source));
                        let value_opt = child.child_by_field_name("value");
                        if let (Some(name), Some(val)) = (name_opt, value_opt) {
                            if matches!(val.kind(), "arrow_function" | "function") {
                                let entity_type = if is_test_name(&name) {
                                    EntityType::TestFunction
                                } else {
                                    EntityType::Function
                                };
                                let entity = CodeEntity::new(
                                    format!("{path}::{name}"),
                                    name,
                                    entity_type,
                                    path.into(),
                                    child.start_position().row as u32 + 1,
                                    child.end_position().row as u32 + 1,
                                    lang,
                                    None,
                                );
                                entities.push((entity, val));
                            }
                        }
                    }
                }
            }
            "class_declaration" | "class" => {
                if let Some(name_node) = node.child_by_field_name("name") {
                    let name = self.node_text(name_node, source);
                    let entity = CodeEntity::new(
                        format!("{path}::{name}"),
                        name.clone(),
                        EntityType::Class,
                        path.into(),
                        node.start_position().row as u32 + 1,
                        node.end_position().row as u32 + 1,
                        lang,
                        None,
                    );
                    entities.push((entity, node));

                    // Collect methods inside the class body
                    if let Some(body) = node.child_by_field_name("body") {
                        let mut body_cursor = body.walk();
                        for member in body.named_children(&mut body_cursor) {
                            if member.kind() == "method_definition" {
                                if let Some(mname_node) = member.child_by_field_name("name") {
                                    let mname = self.node_text(mname_node, source);
                                    let method_type = if is_test_name(&mname) {
                                        EntityType::TestFunction
                                    } else {
                                        EntityType::Method
                                    };
                                    let method = CodeEntity::new(
                                        format!("{path}::{name}.{mname}"),
                                        mname,
                                        method_type,
                                        path.into(),
                                        member.start_position().row as u32 + 1,
                                        member.end_position().row as u32 + 1,
                                        lang,
                                        None,
                                    );
                                    entities.push((method, member));
                                }
                            }
                        }
                    }
                }
            }
            _ => {
                for child in node.named_children(&mut cursor) {
                    entities.extend(self.collect_js_entities(child, source, path, lang));
                }
            }
        }

        entities
    }

    /// Finds call expressions in a JS/TS node subtree.
    fn find_js_calls<'a>(&self, node: Node<'a>, source: &str) -> Vec<String> {
        let mut names = Vec::new();
        let mut cursor = node.walk();

        for child in node.named_children(&mut cursor) {
            if child.kind() == "call_expression" {
                // call_expression → function: identifier | member_expression
                if let Some(func) = child.named_child(0) {
                    let txt = self.node_text(func, source).trim().to_string();
                    if !txt.is_empty() {
                        // Strip `this.` prefix for method calls
                        let name = if let Some(rest) = txt.strip_prefix("this.") {
                            rest.to_string()
                        } else {
                            txt
                        };
                        names.push(name);
                    }
                }
            }
            names.extend(self.find_js_calls(child, source));
        }

        names
    }

    /// Extracts ES module import statements.
    fn extract_js_imports(&self, node: Node, source: &str, file: &str) -> Vec<Import> {
        let mut imports = Vec::new();
        let mut cursor = node.walk();

        for child in node.named_children(&mut cursor) {
            if child.kind() == "import_statement" {
                // import { foo, bar } from './module'
                // import defaultExport from './module'
                let source_str = child
                    .named_children(&mut child.walk())
                    .find(|n| n.kind() == "string")
                    .map(|n| {
                        let raw = self.node_text(n, source);
                        raw.trim_matches('"').trim_matches('\'').to_string()
                    })
                    .unwrap_or_default();

                // Collect imported names from named imports
                let mut cursor2 = child.walk();
                for import_clause in child.named_children(&mut cursor2) {
                    if import_clause.kind() == "import_clause" {
                        let mut clause_cursor = import_clause.walk();
                        for item in import_clause.named_children(&mut clause_cursor) {
                            match item.kind() {
                                "identifier" => {
                                    let name = self.node_text(item, source);
                                    imports.push(Import {
                                        source_file: file.to_string(),
                                        module_path: source_str.clone(),
                                        symbol: Some(name.clone()),
                                        alias: None,
                                        imported_name: name,
                                    });
                                }
                                "named_imports" => {
                                    let mut ni_cursor = item.walk();
                                    for spec in item.named_children(&mut ni_cursor) {
                                        if spec.kind() == "import_specifier" {
                                            let spec_name = spec
                                                .named_child(0)
                                                .map(|n| self.node_text(n, source))
                                                .unwrap_or_default();
                                            let alias = if spec.named_child_count() > 1 {
                                                spec.named_child(1)
                                                    .map(|n| self.node_text(n, source))
                                            } else {
                                                None
                                            };
                                            let imported_name =
                                                alias.clone().unwrap_or_else(|| spec_name.clone());
                                            imports.push(Import {
                                                source_file: file.to_string(),
                                                module_path: source_str.clone(),
                                                symbol: Some(spec_name),
                                                alias,
                                                imported_name,
                                            });
                                        }
                                    }
                                }
                                _ => {}
                            }
                        }
                    }
                }
            }
        }

        imports
    }

    fn extract_python_imports(&self, node: Node, source: &str, file: &str) -> Vec<Import> {
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
                            let alias = child
                                .child_by_field_name("alias")
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
                let module_node = node.child_by_field_name("module_name");
                let module_path = module_node
                    .map(|module| self.node_text(module, source))
                    .unwrap_or_default();
                let module_end = module_node.map(|module| module.end_byte()).unwrap_or(0);
                let mut imports_list = Vec::new();

                for child in node.named_children(&mut cursor) {
                    if module_node
                        .map(|module| {
                            child.start_byte() == module.start_byte()
                                && child.end_byte() == module.end_byte()
                        })
                        .unwrap_or(false)
                    {
                        continue;
                    }

                    if child.kind() == "import_list" {
                        let mut import_cursor = child.walk();
                        for imp in child.named_children(&mut import_cursor) {
                            if imp.kind() == "import_alias" || imp.kind() == "aliased_import" {
                                if let Some(first) = imp.named_child(0) {
                                    let symbol = self.node_text(first, source);
                                    let alias = imp
                                        .child_by_field_name("alias")
                                        .map(|n| self.node_text(n, source));
                                    imports_list.push((symbol, alias));
                                }
                            } else if imp.kind() == "identifier" {
                                let symbol = self.node_text(imp, source);
                                imports_list.push((symbol, None));
                            }
                        }
                    } else if child.kind() == "import_alias" || child.kind() == "aliased_import" {
                        if let Some(first) = child.named_child(0) {
                            let symbol = self.node_text(first, source);
                            let alias = child
                                .child_by_field_name("alias")
                                .map(|n| self.node_text(n, source));
                            imports_list.push((symbol, alias));
                        }
                    } else if child.start_byte() >= module_end
                        && (child.kind() == "dotted_name" || child.kind() == "identifier")
                    {
                        let symbol = self.node_text(child, source);
                        imports_list.push((symbol, None));
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

    fn extract_rust_imports(&self, node: Node, source: &str, file: &str) -> Vec<Import> {
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

    fn parse_rust_use_declaration(&self, node: Node, source: &str, file: &str) -> Vec<Import> {
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
                        .unwrap_or("");
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
        let imported_name = alias
            .clone()
            .unwrap_or_else(|| path.split("::").last().unwrap_or_default().to_string());
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

/// Returns true if `node` (a `function_item`) is preceded by a test-marking
/// attribute: `#[test]`, `#[rstest]`, `#[wasm_bindgen_test]`, or any
/// `#[...::test]` path such as `#[tokio::test]`.
///
/// Tree-sitter-rust places attributes as PRECEDING SIBLINGS of the item they
/// annotate, not as children -so we walk `prev_named_sibling()` while we see
/// `attribute_item` nodes (this also handles stacked attributes like
/// `#[test]\n#[should_panic]\nfn panics() {}`). For each `attribute_item`,
/// `named_child(0)` is the `attribute`, and `attribute.named_child(0)` is the
/// path (`identifier` for `#[test]`, `scoped_identifier` for `#[tokio::test]`).
///
/// We deliberately check only the path, not `arguments` -so
/// `#[cfg(test)] fn helper() {}` (a helper compiled only under test, but not
/// itself a test) is correctly NOT flagged, even though the literal text
/// "test" appears inside its attribute arguments.
fn has_rust_test_attribute(node: Node, source: &str) -> bool {
    let mut sibling = node.prev_named_sibling();

    while let Some(attr_item) = sibling {
        if attr_item.kind() != "attribute_item" {
            break;
        }

        if let Some(path) = attr_item
            .named_child(0)
            .and_then(|attr| attr.named_child(0))
        {
            let path_text = &source[path.start_byte()..path.end_byte()];
            if path_text == "test"
                || path_text == "rstest"
                || path_text == "wasm_bindgen_test"
                || path_text.ends_with("::test")
            {
                return true;
            }
        }

        sibling = attr_item.prev_named_sibling();
    }

    false
}

/// Returns true if `name` follows a common test-function naming convention:
/// snake_case `test_*` (pytest) or camelCase `test*` with an uppercase
/// letter after "test" (Jest/Mocha-style `testLogin`).
///
/// Used for Python/JavaScript/TypeScript, which have no attribute or
/// decorator we can reliably inspect for "this is a test". Rust test
/// detection instead uses `has_rust_test_attribute`, which is more precise
/// (no risk of confusing a function like `testify` with a test).
fn is_test_name(name: &str) -> bool {
    if let Some(rest) = name.strip_prefix("test_") {
        return !rest.is_empty();
    }
    if let Some(rest) = name.strip_prefix("test") {
        return rest.chars().next().is_some_and(|c| c.is_uppercase());
    }
    false
}

/// Returns true if `node` (a Go `function_declaration`) is a test function
/// recognized by `go test`: `func TestXxx(t *testing.T) { ... }`.
///
/// Like `has_rust_test_attribute`, this checks both NAME and SIGNATURE: the
/// name must start with "Test" followed by nothing or an uppercase letter
/// (Go's exported-identifier convention, analogous to `is_test_name`'s
/// camelCase check), AND the function must take a `*testing.T` parameter.
/// Checking the signature too means a helper named `func Testify()` -or a
/// `func TestHelper()` that isn't actually wired up as a test -is correctly
/// NOT flagged.
fn has_go_test_signature(node: Node, name: &str, source: &str) -> bool {
    let Some(rest) = name.strip_prefix("Test") else {
        return false;
    };
    if !(rest.is_empty() || rest.chars().next().is_some_and(|c| c.is_uppercase())) {
        return false;
    }

    let Some(params) = node.child_by_field_name("parameters") else {
        return false;
    };

    let mut cursor = params.walk();
    let has_testing_param = params.named_children(&mut cursor).any(|param| {
        param
            .child_by_field_name("type")
            .map(|ty| &source[ty.start_byte()..ty.end_byte()])
            .is_some_and(|ty_text| ty_text == "*testing.T")
    });
    has_testing_param
}

/// Extracts the receiver type name from a Go `method_declaration`, stripping
/// the leading `*` for pointer receivers.
///
/// `func (a *Auth) Login() {}`  → Some("Auth")
/// `func (s Service) Run() {}`  → Some("Service")
/// `func Standalone() {}`       → None (no receiver field at all)
fn go_receiver_type(node: Node, source: &str) -> Option<String> {
    let receiver = node.child_by_field_name("receiver")?;
    let param = receiver.named_child(0)?;
    let ty = param.child_by_field_name("type")?;
    let ty = if ty.kind() == "pointer_type" {
        ty.named_child(0)?
    } else {
        ty
    };
    Some(source[ty.start_byte()..ty.end_byte()].to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn supported_languages() {
        assert!(CodeParser::new("python").is_ok());
        assert!(CodeParser::new("rs").is_ok());
        assert!(CodeParser::new("auto").is_ok());
        assert!(CodeParser::new("javascript").is_ok());
        assert!(CodeParser::new("typescript").is_ok());
        assert!(CodeParser::new("js").is_ok());
        assert!(CodeParser::new("ts").is_ok());
        assert!(CodeParser::new("go").is_ok());
    }

    #[test]
    fn unsupported_language() {
        assert!(CodeParser::new("cobol").is_err());
    }

    #[test]
    fn parse_javascript_function() {
        let source = "function greet(name) { return name; }\n";
        let entities = CodeParser::new("javascript")
            .unwrap()
            .parse_source("app.js", source)
            .unwrap();
        assert_eq!(entities.len(), 1);
        assert_eq!(entities[0].name, "greet");
        assert_eq!(entities[0].language, Language::JavaScript);
        assert_eq!(entities[0].entity_type, EntityType::Function);
    }

    #[test]
    fn parse_javascript_arrow_function() {
        let source = "const add = (a, b) => a + b;\n";
        let entities = CodeParser::new("javascript")
            .unwrap()
            .parse_source("utils.js", source)
            .unwrap();
        assert_eq!(entities.len(), 1);
        assert_eq!(entities[0].name, "add");
        assert_eq!(entities[0].entity_type, EntityType::Function);
    }

    #[test]
    fn parse_typescript_class() {
        let source = "class Auth {\n  login() {}\n  logout() {}\n}\n";
        let entities = CodeParser::new("typescript")
            .unwrap()
            .parse_source("auth.ts", source)
            .unwrap();
        // 1 class + 2 methods = 3 entities
        assert_eq!(entities.len(), 3);
        let names: Vec<&str> = entities.iter().map(|e| e.name.as_str()).collect();
        assert!(names.contains(&"Auth"));
        assert!(names.contains(&"login"));
        assert!(names.contains(&"logout"));
    }

    #[test]
    fn parse_javascript_class_with_methods() {
        let source = "class Dog {\n  bark() { console.log('woof'); }\n}\n";
        let entities = CodeParser::new("javascript")
            .unwrap()
            .parse_source("dog.js", source)
            .unwrap();
        assert!(entities.iter().any(|e| e.entity_type == EntityType::Class));
        assert!(entities.iter().any(|e| e.entity_type == EntityType::Method));
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

    // ── Test-function detection (Phase 3) ─────────────────────────────────────

    #[test]
    fn rust_test_attribute_detected_as_test_function() {
        let source = "#[test]\nfn it_works() {\n    assert_eq!(1, 1);\n}\n";
        let entities = CodeParser::new("rust")
            .unwrap()
            .parse_source("lib.rs", source)
            .unwrap();
        assert_eq!(entities.len(), 1);
        assert_eq!(entities[0].name, "it_works");
        assert_eq!(entities[0].entity_type, EntityType::TestFunction);
    }

    #[test]
    fn rust_tokio_test_attribute_detected() {
        let source = "#[tokio::test]\nasync fn async_works() {}\n";
        let entities = CodeParser::new("rust")
            .unwrap()
            .parse_source("lib.rs", source)
            .unwrap();
        assert_eq!(entities.len(), 1);
        assert_eq!(entities[0].entity_type, EntityType::TestFunction);
    }

    #[test]
    fn rust_cfg_test_module_nested_test_detected() {
        let source = "#[cfg(test)]\nmod tests {\n    #[test]\n    fn nested() {}\n}\n";
        let entities = CodeParser::new("rust")
            .unwrap()
            .parse_source("lib.rs", source)
            .unwrap();
        let nested = entities.iter().find(|e| e.name == "nested").unwrap();
        assert_eq!(nested.entity_type, EntityType::TestFunction);
    }

    #[test]
    fn rust_function_without_test_attribute_is_plain_function() {
        // #[cfg(test)] on a helper fn must NOT be mistaken for #[test] -the
        // literal "test" appears only inside the attribute's arguments.
        let source = "#[cfg(test)]\nfn helper() {}\n";
        let entities = CodeParser::new("rust")
            .unwrap()
            .parse_source("lib.rs", source)
            .unwrap();
        assert_eq!(entities.len(), 1);
        assert_eq!(entities[0].entity_type, EntityType::Function);
    }

    #[test]
    fn python_test_prefix_function_detected_as_test_function() {
        let source = "def test_login():\n    assert True\n";
        let entities = CodeParser::new("python")
            .unwrap()
            .parse_source("test_auth.py", source)
            .unwrap();
        assert_eq!(entities.len(), 1);
        assert_eq!(entities[0].entity_type, EntityType::TestFunction);
    }

    #[test]
    fn python_non_test_function_is_plain_function() {
        let source = "def login():\n    pass\n";
        let entities = CodeParser::new("python")
            .unwrap()
            .parse_source("auth.py", source)
            .unwrap();
        assert_eq!(entities.len(), 1);
        assert_eq!(entities[0].entity_type, EntityType::Function);
    }

    #[test]
    fn js_test_prefix_function_detected_as_test_function() {
        let source = "function testLogin() { return true; }\n";
        let entities = CodeParser::new("javascript")
            .unwrap()
            .parse_source("auth.test.js", source)
            .unwrap();
        assert_eq!(entities.len(), 1);
        assert_eq!(entities[0].entity_type, EntityType::TestFunction);
    }

    #[test]
    fn parse_go_function() {
        let source = "func Add(a int, b int) int {\n\treturn a + b\n}\n";
        let entities = CodeParser::new("go")
            .unwrap()
            .parse_source("math.go", source)
            .unwrap();
        assert_eq!(entities.len(), 1);
        assert_eq!(entities[0].name, "Add");
        assert_eq!(entities[0].entity_type, EntityType::Function);
        assert_eq!(entities[0].language, Language::Go);
    }

    #[test]
    fn parse_go_method_with_pointer_receiver() {
        let source = "type Auth struct{}\n\nfunc (a *Auth) Login() bool {\n\treturn true\n}\n";
        let entities = CodeParser::new("go")
            .unwrap()
            .parse_source("auth.go", source)
            .unwrap();
        assert_eq!(entities.len(), 1);
        assert_eq!(entities[0].id, "auth.go::Auth::Login");
        assert_eq!(entities[0].entity_type, EntityType::Method);
    }

    #[test]
    fn go_test_function_detected_as_test_function() {
        let source =
            "import \"testing\"\n\nfunc TestLogin(t *testing.T) {\n\tif !true {\n\t\tt.Fail()\n\t}\n}\n";
        let entities = CodeParser::new("go")
            .unwrap()
            .parse_source("auth_test.go", source)
            .unwrap();
        assert_eq!(entities.len(), 1);
        assert_eq!(entities[0].entity_type, EntityType::TestFunction);
    }

    #[test]
    fn go_function_without_testing_param_is_plain_function() {
        let source = "func TestHelper() {\n\tdoSetup()\n}\n";
        let entities = CodeParser::new("go")
            .unwrap()
            .parse_source("helpers.go", source)
            .unwrap();
        assert_eq!(entities.len(), 1);
        assert_eq!(entities[0].entity_type, EntityType::Function);
    }

    #[test]
    fn go_call_expression_resolves_to_local_function() {
        let source = "func main() {\n\thelper()\n}\n\nfunc helper() {}\n";
        let entities = CodeParser::new("go")
            .unwrap()
            .parse_source("main.go", source)
            .unwrap();
        assert_eq!(entities.len(), 2);
        let main_entity = entities.iter().find(|e| e.name == "main").unwrap();
        assert!(main_entity.calls.contains(&"main.go::helper".to_string()));
    }

    #[test]
    fn go_import_extracted() {
        let source =
            "package main\n\nimport (\n\t\"fmt\"\n\tmyalias \"path/to/pkg\"\n)\n\nfunc main() {}\n";
        let imports = CodeParser::new("go")
            .unwrap()
            .extract_imports("main.go", source)
            .unwrap();
        assert_eq!(imports.len(), 2);
        assert!(imports
            .iter()
            .any(|i| i.module_path == "fmt" && i.imported_name == "fmt"));
        assert!(imports.iter().any(|i| i.module_path == "path/to/pkg"
            && i.alias.as_deref() == Some("myalias")
            && i.imported_name == "myalias"));
    }
}
