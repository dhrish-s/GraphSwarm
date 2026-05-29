/// Helpers to generate consistent KV keys.
pub struct StorageSchema;

impl StorageSchema {
    pub fn entity_key(entity_id: &str) -> String {
        format!("callgraph:{entity_id}")
    }
    pub fn file_index_key(file: &str) -> String {
        format!("file_index:{file}")
    }
    pub fn import_key(file: &str) -> String {
        format!("import_graph:{file}")
    }
    pub fn action_key(timestamp_nanos: &str) -> String {
        format!("agent:action:{timestamp_nanos}")
    }
    pub fn task_key(task_id: &str) -> String {
        format!("agent:task:{task_id}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn key_formats() {
        assert_eq!(StorageSchema::entity_key("a.py::foo"), "callgraph:a.py::foo");
        assert_eq!(StorageSchema::action_key("123"), "agent:action:123");
    }
}
