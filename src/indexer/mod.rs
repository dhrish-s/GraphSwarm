pub mod parser;
pub mod call_graph;
pub mod extractor;

pub use parser::{CodeParser, Import};
pub use call_graph::CallGraph;
pub use extractor::{CodeEntity, EntityType};

use crate::error::Result;
use std::path::Path;
use std::collections::HashMap;

#[derive(Debug)]
struct ImportResolution {
    target_module: String,
    target_symbol: Option<String>,
    imported_name: String,
}

/// Top-level indexer that parses a repo and produces a CallGraph.
pub struct CodeIndexer {
    parser: CodeParser,
}

impl CodeIndexer {
    pub fn new(language: &str) -> Result<Self> {
        Ok(Self { parser: CodeParser::new(language)? })
    }

    /// Index every source file under `path` and build a call graph with cross-file resolution.
    pub fn index_directory(&self, _path: impl AsRef<Path>) -> Result<CallGraph> {
        let path = _path.as_ref();
        let mut graph = CallGraph::new();
        graph.set_repo_path(path.to_string_lossy().into_owned());

        // First pass: collect all entities and imports
        let mut all_entities: Vec<CodeEntity> = Vec::new();
        let mut all_imports: Vec<Import> = Vec::new();
        let mut file_to_module: HashMap<String, String> = HashMap::new();

        fn visit_dir(
            indexer: &CodeIndexer,
            dir: &Path,
            root: &Path,
            all_entities: &mut Vec<CodeEntity>,
            all_imports: &mut Vec<Import>,
            file_to_module: &mut HashMap<String, String>,
        ) -> Result<()> {
            for entry in std::fs::read_dir(dir)? {
                let entry = entry?;
                let p = entry.path();
                if p.is_dir() {
                    visit_dir(indexer, &p, root, all_entities, all_imports, file_to_module)?;
                    continue;
                }

                let content = match std::fs::read_to_string(&p) {
                    Ok(s) => s,
                    Err(_) => continue,
                };

                let p_str = p.to_string_lossy().to_string();
                if let Ok(entities) = indexer.parser.parse_source(&p_str, &content) {
                    all_entities.extend(entities);
                }

                if let Ok(imports) = indexer.parser.extract_imports(&p_str, &content) {
                    all_imports.extend(imports);
                }

                let mut module_path = p.strip_prefix(root)
                    .unwrap_or(&p)
                    .with_extension("")
                    .to_string_lossy()
                    .to_string();

                if let Some(stem) = p.file_stem() {
                    if stem == "__init__" || stem == "mod" {
                        module_path = p.strip_prefix(root)
                            .unwrap_or(&p)
                            .parent()
                            .map(|parent| parent.to_string_lossy().to_string())
                            .unwrap_or_default();
                    }
                }

                let module_name = module_path
                    .trim_start_matches(std::path::MAIN_SEPARATOR)
                    .replace(std::path::MAIN_SEPARATOR, ".");

                file_to_module.insert(p_str, module_name);
            }
            Ok(())
        }

        visit_dir(
            self,
            path,
            path,
            &mut all_entities,
            &mut all_imports,
            &mut file_to_module,
        )?;

        // Add all entities to graph and add intra-file calls
        for e in &all_entities {
            graph.add_entity(e.clone());
            // Add intra-file call edges (already computed by parser)
            // Only add calls that are resolved IDs (contain ::), not unresolved names
            for call_id in &e.calls {
                if call_id.contains("::") {
                    graph.add_call(e.id.clone(), call_id.clone());
                }
            }
        }

        // Second pass: resolve cross-file calls using imports
        let symbol_table = self.build_symbol_table(&all_entities, &file_to_module);
        self.resolve_cross_file_calls(&mut graph, &all_entities, &all_imports, &symbol_table);

        Ok(graph)
    }

    /// Build a symbol table: (module_name, symbol_name) -> entity_id
    fn build_symbol_table(
        &self,
        entities: &[CodeEntity],
        file_to_module: &HashMap<String, String>,
    ) -> HashMap<(String, String), String> {
        let mut symbol_table: HashMap<(String, String), String> = HashMap::new();

        for entity in entities {
            // Get the module name from file path
            if let Some(module) = file_to_module.get(&entity.file_path) {
                // Map module::name -> entity_id
                symbol_table.insert((module.clone(), entity.name.clone()), entity.id.clone());
            }
        }

        symbol_table
    }

    /// Resolve cross-file calls using imports
    fn resolve_cross_file_calls(
        &self,
        graph: &mut CallGraph,
        entities: &[CodeEntity],
        imports: &[Import],
        symbol_table: &HashMap<(String, String), String>,
    ) {
        let mut import_map: HashMap<(String, String), ImportResolution> = HashMap::new();

        for imp in imports {
            let imported_name = imp.imported_name.clone();
            let key = (imp.source_file.clone(), imported_name.clone());
            import_map.insert(
                key,
                ImportResolution {
                    target_module: imp.module_path.clone(),
                    target_symbol: imp.symbol.clone(),
                    imported_name,
                },
            );
        }

        for entity in entities {
            for call_name in entity.calls.clone() {
                if let Some(resolution) = import_map.get(&(entity.file_path.clone(), call_name.clone())) {
                    if let Some(target_id) = self.resolve_direct_import(symbol_table, resolution) {
                        graph.add_call(entity.id.clone(), target_id);
                    }
                }

                if let Some((prefix, separator)) = Self::extract_call_prefix(&call_name) {
                    if let Some(resolution) = import_map.get(&(entity.file_path.clone(), prefix.to_string())) {
                        if let Some(target_id) = self.resolve_qualified_import(symbol_table, resolution, &call_name, separator) {
                            graph.add_call(entity.id.clone(), target_id);
                        }
                    }
                }
            }
        }
    }

    fn resolve_direct_import(
        &self,
        symbol_table: &HashMap<(String, String), String>,
        resolution: &ImportResolution,
    ) -> Option<String> {
        if let Some(symbol) = &resolution.target_symbol {
            let module = if resolution.target_module.is_empty() {
                resolution.imported_name.clone()
            } else {
                resolution.target_module.clone()
            };
            symbol_table.get(&(module, symbol.clone())).cloned()
        } else {
            None
        }
    }

    fn extract_call_prefix(call_name: &str) -> Option<(&str, &str)> {
        if call_name.contains("::") {
            call_name.split_once("::").map(|(prefix, _)| (prefix, "::"))
        } else if call_name.contains('.') {
            call_name.split_once('.').map(|(prefix, _)| (prefix, "."))
        } else {
            None
        }
    }

    fn resolve_qualified_import(
        &self,
        symbol_table: &HashMap<(String, String), String>,
        resolution: &ImportResolution,
        call_name: &str,
        separator: &str,
    ) -> Option<String> {
        let segments: Vec<&str> = call_name.split(separator).filter(|s| !s.is_empty()).collect();
        if segments.len() < 2 || segments[0] != resolution.imported_name {
            return None;
        }

        let suffix_segments = &segments[1..];
        let mut target_module = resolution.target_module.clone();
        if let Some(symbol) = &resolution.target_symbol {
            if !target_module.is_empty() {
                target_module = format!("{target_module}{separator}{symbol}");
            }
        }

        let mut resolved_symbol = suffix_segments.last()?.to_string();
        let module_segments: Vec<&str> = target_module.split(separator).filter(|s| !s.is_empty()).collect();
        let suffix: Vec<&str> = suffix_segments.to_vec();

        if module_segments.len() > 1 {
            let compare = &module_segments[1..];
            if !compare.is_empty() && suffix.starts_with(compare) && suffix.len() > compare.len() {
                resolved_symbol = suffix[compare.len()..].last()?.to_string();
            }
        }

        symbol_table.get(&(target_module, resolved_symbol)).cloned()
    }

    pub fn language(&self) -> &str {
        self.parser.language()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;
    use std::io::Write;

    #[test]
    fn create_indexer() {
        assert!(CodeIndexer::new("python").is_ok());
        assert!(CodeIndexer::new("cobol").is_err());
    }

    #[test]
    fn same_file_calls() {
        let dir = tempdir().unwrap();
        let file = dir.path().join("test.py");
        let mut f = std::fs::File::create(&file).unwrap();
        writeln!(f, "def foo():\n    pass\n\ndef bar():\n    foo()\n").unwrap();

        let indexer = CodeIndexer::new("auto").unwrap();
        let graph = indexer.index_directory(dir.path()).unwrap();

        assert_eq!(graph.entity_count(), 2);
        assert_eq!(graph.edge_count(), 1);
    }

    #[test]
    fn python_from_import_resolution() {
        let dir = tempdir().unwrap();

        // payment.py
        let file1 = dir.path().join("payment.py");
        let mut f1 = std::fs::File::create(&file1).unwrap();
        writeln!(f1, "def process_payment():\n    pass\n").unwrap();

        // checkout.py
        let file2 = dir.path().join("checkout.py");
        let mut f2 = std::fs::File::create(&file2).unwrap();
        writeln!(f2, "from payment import process_payment\n\ndef checkout():\n    process_payment()\n").unwrap();

        let indexer = CodeIndexer::new("auto").unwrap();
        let graph = indexer.index_directory(dir.path()).unwrap();

        // Should have 2 entities and at least 1 edge (cross-file call)
        assert_eq!(graph.entity_count(), 2);
        assert!(graph.edge_count() > 0, "Expected cross-file call edge");
    }

    #[test]
    fn python_alias_import_resolution() {
        let dir = tempdir().unwrap();

        // utils.py
        let file1 = dir.path().join("utils.py");
        let mut f1 = std::fs::File::create(&file1).unwrap();
        writeln!(f1, "def helper():\n    pass\n").unwrap();

        // main.py
        let file2 = dir.path().join("main.py");
        let mut f2 = std::fs::File::create(&file2).unwrap();
        writeln!(f2, "from utils import helper as h\n\ndef run():\n    h()\n").unwrap();

        let indexer = CodeIndexer::new("auto").unwrap();
        let graph = indexer.index_directory(dir.path()).unwrap();

        assert_eq!(graph.entity_count(), 2);
        // Alias resolution should find the call
        assert!(graph.edge_count() > 0);
    }

    #[test]
    fn python_module_import_resolution() {
        let dir = tempdir().unwrap();

        let file1 = dir.path().join("payment.py");
        let mut f1 = std::fs::File::create(&file1).unwrap();
        writeln!(f1, "def process_payment():\n    pass\n").unwrap();

        let file2 = dir.path().join("checkout.py");
        let mut f2 = std::fs::File::create(&file2).unwrap();
        writeln!(f2, "import payment\n\ndef checkout():\n    payment.process_payment()\n").unwrap();

        let indexer = CodeIndexer::new("auto").unwrap();
        let graph = indexer.index_directory(dir.path()).unwrap();

        assert_eq!(graph.entity_count(), 2);
        assert!(graph.edge_count() > 0, "Expected module-qualified imported call edge");
    }

    #[test]
    fn rust_use_import_resolution() {
        let dir = tempdir().unwrap();

        let file1 = dir.path().join("foo.rs");
        let mut f1 = std::fs::File::create(&file1).unwrap();
        writeln!(f1, "fn helper() {{}}\n").unwrap();

        let file2 = dir.path().join("main.rs");
        let mut f2 = std::fs::File::create(&file2).unwrap();
        writeln!(f2, "use foo::helper;\n\nfn run() {{\n    helper();\n}}\n").unwrap();

        let indexer = CodeIndexer::new("auto").unwrap();
        let graph = indexer.index_directory(dir.path()).unwrap();

        assert_eq!(graph.entity_count(), 2);
        assert!(graph.edge_count() > 0, "Expected Rust use-import cross-file edge");
    }

    #[test]
    fn unresolved_calls_safe() {
        let dir = tempdir().unwrap();

        // test.py
        let file = dir.path().join("test.py");
        let mut f = std::fs::File::create(&file).unwrap();
        writeln!(f, "def foo():\n    undefined_func()\n").unwrap();

        let indexer = CodeIndexer::new("auto").unwrap();
        let graph = indexer.index_directory(dir.path()).unwrap();

        // Should not crash, entity is present
        assert_eq!(graph.entity_count(), 1);
        // Unresolved calls should not create edges
        assert_eq!(graph.edge_count(), 0);
    }
}
