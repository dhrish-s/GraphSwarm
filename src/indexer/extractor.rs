use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum EntityType {
    Function,
    Class,
    Method,
    Module,
}

impl std::fmt::Display for EntityType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            EntityType::Function => write!(f, "function"),
            EntityType::Class => write!(f, "class"),
            EntityType::Method => write!(f, "method"),
            EntityType::Module => write!(f, "module"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CodeEntity {
    pub id: String,
    pub name: String,
    pub file: String,
    pub entity_type: EntityType,
    pub signature: String,
    pub calls: Vec<String>,
    pub called_by: Vec<String>,
    pub imports: Vec<String>,
    pub imported_by: Vec<String>,
    pub line_number: usize,
    pub metadata: HashMap<String, String>,
}

impl CodeEntity {
    pub fn new(
        id: String,
        name: String,
        file: String,
        entity_type: EntityType,
        signature: String,
        line_number: usize,
    ) -> Self {
        Self {
            id, name, file, entity_type, signature,
            calls: Vec::new(),
            called_by: Vec::new(),
            imports: Vec::new(),
            imported_by: Vec::new(),
            line_number,
            metadata: HashMap::new(),
        }
    }

    pub fn add_call(&mut self, target_id: String) {
        if !self.calls.contains(&target_id) {
            self.calls.push(target_id);
        }
    }

    pub fn add_called_by(&mut self, caller_id: String) {
        if !self.called_by.contains(&caller_id) {
            self.called_by.push(caller_id);
        }
    }

    pub fn add_import(&mut self, import: String) {
        if !self.imports.contains(&import) {
            self.imports.push(import);
        }
    }

    pub fn add_imported_by(&mut self, importer: String) {
        if !self.imported_by.contains(&importer) {
            self.imported_by.push(importer);
        }
    }

    pub fn set_metadata(&mut self, key: String, value: String) {
        self.metadata.insert(key, value);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn create_entity() {
        let e = CodeEntity::new(
            "a.py::foo".into(), "foo".into(), "a.py".into(),
            EntityType::Function, "def foo()".into(), 1,
        );
        assert_eq!(e.name, "foo");
        assert_eq!(e.entity_type, EntityType::Function);
    }

    #[test]
    fn no_duplicate_calls() {
        let mut e = CodeEntity::new(
            "a.py::foo".into(), "foo".into(), "a.py".into(),
            EntityType::Function, "def foo()".into(), 1,
        );
        e.add_call("b.py::bar".into());
        e.add_call("b.py::bar".into());
        assert_eq!(e.calls.len(), 1);
    }

    #[test]
    fn entity_type_display() {
        assert_eq!(format!("{}", EntityType::Class), "class");
    }
}
