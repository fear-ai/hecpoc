pub fn is_valid_index_name(index: &str, max_len: usize) -> bool {
    if index.is_empty() || index.len() > max_len {
        return false;
    }
    let Some(first) = index.chars().next() else {
        return false;
    };
    if first == '_' || first == '-' {
        return false;
    }
    if index.contains("kvstore") {
        return false;
    }
    index.bytes().all(|byte| {
        byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_' || byte == b'-'
    })
}

#[cfg(test)]
mod tests {
    use super::is_valid_index_name;

    #[test]
    fn validates_supported_index_names() {
        assert!(is_valid_index_name("main", 128));
        assert!(is_valid_index_name("app_logs-1", 128));
    }

    #[test]
    fn rejects_unsupported_index_names() {
        assert!(!is_valid_index_name("", 128));
        assert!(!is_valid_index_name("_internal", 128));
        assert!(!is_valid_index_name("-dash", 128));
        assert!(!is_valid_index_name("Bad.Index", 128));
        assert!(!is_valid_index_name("my_kvstore_logs", 128));
        assert!(!is_valid_index_name("abcd", 3));
    }
}
