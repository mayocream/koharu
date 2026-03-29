use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileEntry {
    pub name: String,
    #[serde(with = "serde_bytes")]
    pub data: Vec<u8>,
}

#[cfg(test)]
mod tests {
    use super::FileEntry;

    #[test]
    fn file_entry_round_trips() {
        let value = FileEntry {
            name: "page.png".to_string(),
            data: vec![7, 8, 9],
        };

        let encoded = serde_json::to_vec(&value).expect("serialize");
        let decoded: FileEntry = serde_json::from_slice(&encoded).expect("deserialize");

        assert_eq!(decoded.name, "page.png");
        assert_eq!(decoded.data, vec![7, 8, 9]);
    }
}
