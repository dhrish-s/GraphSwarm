use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum EntityType {
    Function,
    Class,
    Method,
    Import,
    Module,
    TestFunction,
}

impl std::fmt::Display for EntityType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            EntityType::Function => write!(f, "function"),
            EntityType::Class => write!(f, "class"),
            EntityType::Method => write!(f, "method"),
            EntityType::Import => write!(f, "import"),
            EntityType::Module => write!(f, "module"),
            EntityType::TestFunction => write!(f, "test_function"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Hash)]
pub enum Language {
    Rust,
    Python,
    JavaScript,
    TypeScript,
    Go,
    Unknown,
}

impl std::fmt::Display for Language {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Language::Rust => write!(f, "rust"),
            Language::Python => write!(f, "python"),
            Language::JavaScript => write!(f, "javascript"),
            Language::TypeScript => write!(f, "typescript"),
            Language::Go => write!(f, "go"),
            Language::Unknown => write!(f, "unknown"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CodeEntity {
    pub id: String,
    pub name: String,
    pub entity_type: EntityType,
    pub file_path: String,
    pub line_start: u32,
    pub line_end: u32,
    pub language: Language,
    pub docstring: Option<String>,
    pub calls: Vec<String>,
    pub called_by: Vec<String>,
}

impl CodeEntity {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        id: String,
        name: String,
        entity_type: EntityType,
        file_path: String,
        line_start: u32,
        line_end: u32,
        language: Language,
        docstring: Option<String>,
    ) -> Self {
        Self {
            id,
            name,
            entity_type,
            file_path,
            line_start,
            line_end,
            language,
            docstring,
            calls: Vec::new(),
            called_by: Vec::new(),
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
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn create_entity() {
        let e = CodeEntity::new(
            "a.py::foo".into(),
            "foo".into(),
            EntityType::Function,
            "a.py".into(),
            1,
            1,
            Language::Python,
            Some("docs".into()),
        );

        assert_eq!(e.name, "foo");
        assert_eq!(e.entity_type, EntityType::Function);
        assert_eq!(e.language, Language::Python);
        assert_eq!(e.docstring.as_deref(), Some("docs"));
    }

    #[test]
    fn no_duplicate_calls() {
        let mut e = CodeEntity::new(
            "a.py::foo".into(),
            "foo".into(),
            EntityType::Function,
            "a.py".into(),
            1,
            1,
            Language::Python,
            None,
        );
        e.add_call("b.py::bar".into());
        e.add_call("b.py::bar".into());
        assert_eq!(e.calls.len(), 1);
    }

    #[test]
    fn entity_type_display() {
        assert_eq!(format!("{}", EntityType::Class), "class");
    }

    #[test]
    fn entity_type_display_test_function() {
        assert_eq!(format!("{}", EntityType::TestFunction), "test_function");
    }
}
